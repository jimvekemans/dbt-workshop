use regex::Regex;
use serde::{Deserialize, Serialize};
use vdp_git::WriteGuard;

use crate::VsError;

/// `[vaultspeed_release]`: the branches of the vaultspeed-automation-framework repository.
///
/// `main` is the neutral skeleton (no DAGs, no SQL state); every environment branch is based on it and carries its
/// own deploys on top; VaultSpeed's agent pushes one `release/<n>_<TYPE>` branch per zip; a real release is a
/// `feature/<DROP>` branch collecting zips and PML scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct VsReleaseSettings {
    /// Environment branches, lowest first.
    pub environments: Vec<String>,
    /// Environments CI feeds itself (every zip branch is merged automatically).
    pub auto_environments: Vec<String>,
    /// Environments that may receive single zip branches directly (others only release branches).
    pub direct_merge_environments: Vec<String>,
    pub main: String,
    /// Branches VaultSpeed's agent creates, one per zip; never written by the tools.
    pub zip_prefix: String,
    /// Release branches (`feature/2026_DROP25.1`).
    pub release_prefix: String,
    pub ticket_prefix: String,
    pub staging_dir: String,
    pub pml_dir: String,
    pub archive_dir: String,
    pub logs_dir: String,
    /// Paths a commit on `main` may never touch: main stays the neutral skeleton.
    pub skeleton_excluded: Vec<String>,
}

impl Default for VsReleaseSettings {
    fn default() -> Self {
        Self {
            environments: ["LDV", "ABT", "CBT", "PRD"].map(String::from).to_vec(),
            auto_environments: vec!["LDV".into()],
            direct_merge_environments: vec!["LDV".into(), "ABT".into()],
            main: "main".into(),
            zip_prefix: "release/".into(),
            release_prefix: "feature/".into(),
            ticket_prefix: "INTEL".into(),
            staging_dir: "vaultspeed/staging/release".into(),
            pml_dir: "vaultspeed/staging/pre_model_input".into(),
            archive_dir: "vaultspeed/deployed/archive".into(),
            logs_dir: "vaultspeed/logs".into(),
            // The rewrite rules (`vaultspeed/staging/manual_changes`) are skeleton; staged work and state are not.
            skeleton_excluded: [
                "vaultspeed/staging/release/",
                "vaultspeed/staging/pre_model_input/",
                "vaultspeed/deployed/",
                "vaultspeed/airflow/",
                "vaultspeed/ledger/",
                "vaultspeed/logs/",
                "logs/",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

impl VsReleaseSettings {
    pub const SECTION: &'static str = "vaultspeed_release";
}

/// The branch rules derived from the settings.
#[derive(Debug, Clone)]
pub struct VsRules {
    pub settings: VsReleaseSettings,
}

/// `2026_DROP25` or `2026_DROP25.1` (the `.n` numbers VaultSpeed releases within one DROP).
fn drop_pattern() -> Regex {
    Regex::new(r"^(\d{4}_E?DROP\d{1,2})(\.\d{1,3})?$").expect("valid pattern")
}

impl VsRules {
    pub fn new(settings: VsReleaseSettings) -> Self {
        Self { settings }
    }

    pub fn environments(&self) -> &[String] {
        &self.settings.environments
    }

    pub fn is_environment(&self, branch: &str) -> bool {
        self.settings.environments.iter().any(|e| e == branch)
    }

    /// The environment below `environment` (`None` for the lowest).
    pub fn lower(&self, environment: &str) -> Option<&str> {
        let index = self
            .settings
            .environments
            .iter()
            .position(|e| e == environment)?;
        index
            .checked_sub(1)
            .map(|i| self.settings.environments[i].as_str())
    }

    pub fn is_zip_branch(&self, branch: &str) -> bool {
        branch.starts_with(&self.settings.zip_prefix)
            && branch.len() > self.settings.zip_prefix.len()
    }

    pub fn is_release_branch(&self, branch: &str) -> bool {
        branch.starts_with(&self.settings.release_prefix)
            && branch.len() > self.settings.release_prefix.len()
    }

    /// The release branch for a DROP name (`feature/2026_DROP25.1`).
    pub fn release_branch(&self, drop: &str) -> Result<String, VsError> {
        let drop = drop.trim();
        if !drop_pattern().is_match(drop) {
            return Err(VsError::Invalid(format!(
                "`{drop}` is not a release name (expected e.g. 2026_DROP25 or 2026_DROP25.1)"
            )));
        }
        Ok(format!("{}{drop}", self.settings.release_prefix))
    }

    /// The Jira fix version of a release name: `2026_DROP25.1` → `2026_DROP25`.
    pub fn fix_version(drop: &str) -> Option<String> {
        drop_pattern()
            .captures(drop.trim())
            .map(|c| c[1].to_owned())
    }

    /// What the tools may write: release branches, the environments and main (for the skeleton). Zip branches
    /// are VaultSpeed's and are never written.
    pub fn guard(&self) -> WriteGuard {
        let mut writable = vec![self.settings.release_prefix.clone()];
        writable.extend(self.settings.environments.iter().cloned());
        writable.push(self.settings.main.clone());
        WriteGuard {
            protected: Vec::new(),
            writable,
        }
    }

    /// The message GitLab gives a merge request, which the framework's CI deploy rule reacts to.
    pub fn merge_message(source: &str, target: &str) -> String {
        format!("Merge branch '{source}' into '{target}'")
    }

    /// Whether a commit message would make CI deploy (merges and auto-merges do; a direct fix must not).
    pub fn triggers_deploy(message: &str) -> bool {
        let first = message.trim_start();
        first.starts_with("Merge branch") || first.starts_with("Auto-merge")
    }

    /// `Err` when a commit on `main` would touch a path that belongs to the environments.
    pub fn check_skeleton<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), VsError> {
        let excluded: Vec<&str> = paths
            .into_iter()
            .filter(|p| {
                self.settings
                    .skeleton_excluded
                    .iter()
                    .any(|x| p.starts_with(x.as_str()))
            })
            .collect();
        if excluded.is_empty() {
            Ok(())
        } else {
            Err(VsError::Refused(format!(
                "{} stays the neutral skeleton: {} belong(s) to the environments",
                self.settings.main,
                excluded.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> VsRules {
        VsRules::new(VsReleaseSettings::default())
    }

    #[test]
    fn release_names_and_fix_versions() {
        assert_eq!(
            rules().release_branch("2026_DROP25.1").unwrap(),
            "feature/2026_DROP25.1"
        );
        assert_eq!(
            rules().release_branch(" 2026_EDROP3 ").unwrap(),
            "feature/2026_EDROP3"
        );
        for bad in ["DROP25", "2026_DROP25.x", "dev", "../x", "2026_DROP25.1/x"] {
            assert!(rules().release_branch(bad).is_err(), "{bad}");
        }
        assert_eq!(
            VsRules::fix_version("2026_DROP25.2").as_deref(),
            Some("2026_DROP25")
        );
    }

    #[test]
    fn the_guard_never_lets_zip_branches_be_written() {
        let guard = rules().guard();
        for ok in ["feature/2026_DROP25.1", "LDV", "CBT", "main"] {
            assert!(guard.check(ok).is_ok(), "{ok}");
        }
        for refused in ["release/1182_FMC", "dev", "feature/", "prod"] {
            assert!(guard.check(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn environments_order_and_skeleton() {
        let rules = rules();
        assert_eq!(rules.lower("CBT"), Some("ABT"));
        assert_eq!(rules.lower("LDV"), None);
        assert!(VsRules::triggers_deploy("Merge branch 'x' into 'ABT'"));
        assert!(!VsRules::triggers_deploy(
            "Fix QUERY_MODIFICATIONS for _ sources"
        ));
        assert!(
            rules
                .check_skeleton([
                    "config/vaultspeed.json",
                    ".gitlab-ci.yml",
                    "vaultspeed/staging/manual_changes/DDL/QUERY_MODIFICATIONS.json",
                ])
                .is_ok()
        );
        assert!(
            rules
                .check_skeleton(["vaultspeed/deployed/archive/1_FMC.zip"])
                .is_err()
        );
    }
}
