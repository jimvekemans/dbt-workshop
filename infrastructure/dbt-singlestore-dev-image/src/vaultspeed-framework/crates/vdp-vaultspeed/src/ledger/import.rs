use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use chrono::Utc;
use vdp_vaultspeed_domain::{ArtifactKind, JobName, LedgerEntry, LedgerOutcome, RelPath, RunId};
use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, RoutineAwareSplitter};
use walkdir::WalkDir;

use crate::ledger::{LedgerError, LedgerWriter};

/// Counts per artifact kind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub files: BTreeMap<ArtifactKind, usize>,
    pub statements: BTreeMap<ArtifactKind, usize>,
}

/// Seeds a ledger from the Python framework's `vaultspeed/deployed/{DDL,ETL,PML}` files.
///
/// Those files contain the *executed* (post-rewrite) SQL, appended run after run, so
/// every statement becomes an `Imported` entry with only an effective fingerprint, in
/// file order: the last version of an object in a file is its latest version.
#[derive(Debug)]
pub struct LegacyImporter {
    pipeline: ArtifactPipeline,
}

impl Default for LegacyImporter {
    fn default() -> Self {
        Self {
            pipeline: ArtifactPipeline::new(
                Arc::new(RoutineAwareSplitter),
                vec![],
                Arc::new(NoRewrite),
            ),
        }
    }
}

impl LegacyImporter {
    /// The `source` stamped on imported entries.
    pub const SOURCE: &'static str = "legacy:deployed";

    /// Imports every `.sql` file under `deployed_dir/{PML,DDL,ETL}` into `ledger` as `Imported` entries.
    ///
    /// # Errors
    ///
    /// [`LedgerError::Io`] when a file cannot be read; [`LedgerError::Legacy`] when a file cannot be split into statements;
    /// the errors of `ledger` when an entry cannot be recorded.
    pub fn import(
        &self,
        deployed_dir: &Path,
        run_id: &RunId,
        ledger: &dyn LedgerWriter,
    ) -> Result<ImportStats, LedgerError> {
        let mut stats = ImportStats::default();
        for artifact in [ArtifactKind::Pml, ArtifactKind::Ddl, ArtifactKind::Etl] {
            let root = deployed_dir.join(artifact.as_str());
            if !root.is_dir() {
                continue;
            }
            let mut files: Vec<_> = WalkDir::new(&root)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(walkdir::DirEntry::into_path)
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")))
                .collect();
            files.sort();
            for path in files {
                let count = self.import_file(&root, &path, artifact, run_id, ledger)?;
                *stats.files.entry(artifact).or_default() += 1;
                *stats.statements.entry(artifact).or_default() += count;
            }
        }
        Ok(stats)
    }

    fn import_file(
        &self,
        root: &Path,
        path: &Path,
        artifact: ArtifactKind,
        run_id: &RunId,
        ledger: &dyn LedgerWriter,
    ) -> Result<usize, LedgerError> {
        let bytes = fs::read(path).map_err(|source| LedgerError::Io {
            path: path.to_owned(),
            source,
        })?;
        let text = String::from_utf8_lossy(&bytes);
        let rel_path = RelPath::new(path.strip_prefix(root).unwrap_or(path).to_string_lossy());
        let job = JobName::from_file_stem(rel_path.file_stem());
        let statements = self
            .pipeline
            .statements(&text)
            .map_err(|source| LedgerError::Legacy {
                path: path.to_owned(),
                source,
            })?;
        for statement in &statements {
            ledger.record(LedgerEntry {
                sequence: 0,
                run_id: run_id.clone(),
                recorded_at: Utc::now(),
                source: Self::SOURCE.to_owned(),
                job: job.to_string(),
                rel_path: format!("{artifact}/{rel_path}"),
                object_key: statement.object_key().map(|k| k.to_string()),
                statement_kind: statement.kind.label(),
                raw_fp: None,
                effective_fp: statement.effective_fp,
                outcome: LedgerOutcome::Imported,
                detail: None,
                git_sha: None,
                renamed_to: None,
            })?;
        }
        Ok(statements.len())
    }
}

#[cfg(test)]
mod tests {
    use vdp_vaultspeed_domain::Fingerprint;
    use vdp_vaultspeed_sql::fingerprint;

    use super::*;
    use crate::ledger::{LedgerReader, MemoryLedger};

    #[test]
    fn imports_appended_history_with_latest_version_last() {
        let dir = tempfile::tempdir().unwrap();
        let etl = dir.path().join("ETL/INCR/FOUNDATION_LAYER");
        fs::create_dir_all(&etl).unwrap();
        fs::write(
            etl.join("HUB_X_INCR.sql"),
            "CREATE OR REPLACE PROCEDURE `edw_proc`.`hub_x_incr`() AS BEGIN SELECT 1; END;\n\
             CREATE OR REPLACE PROCEDURE `edw_proc`.`hub_x_incr`() AS BEGIN SELECT 2; END;",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("DDL")).unwrap();
        fs::write(
            dir.path().join("DDL/GENERATE_DDL_ALTER_DV_EDW_DV_RDV.sql"),
            "ALTER TABLE `edw_dv_rdv`.`ref_k`\n\tADD COLUMN `kat_3p` VARCHAR(6);",
        )
        .unwrap();

        let ledger = MemoryLedger::new();
        let stats = LegacyImporter::default()
            .import(dir.path(), &RunId::from_string("import"), &ledger)
            .unwrap();
        assert_eq!(stats.statements[&ArtifactKind::Etl], 2);
        assert_eq!(stats.statements[&ArtifactKind::Ddl], 1);

        let latest = ledger
            .latest_for_object("procedure:edw_proc.hub_x_incr")
            .unwrap()
            .unwrap();
        assert_eq!(
            latest.effective_fp,
            fingerprint(
                "CREATE OR REPLACE PROCEDURE `edw_proc`.`hub_x_incr`() AS BEGIN SELECT 2; END"
            )
        );
        let alter = fingerprint("ALTER TABLE edw_dv_rdv.ref_k ADD COLUMN kat_3p VARCHAR(6)");
        assert!(
            ledger
                .find_applied(Some(&Fingerprint::of("unknown raw")), &alter)
                .unwrap()
                .is_some()
        );
    }
}
