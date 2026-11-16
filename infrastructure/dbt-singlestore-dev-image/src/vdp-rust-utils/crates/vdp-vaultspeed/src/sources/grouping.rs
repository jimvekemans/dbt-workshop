use std::collections::HashMap;
use std::fmt::Debug;

use vdp_vaultspeed_domain::Statement;

/// Partitions the statements of one file into execution units, preserving order.
pub trait GroupingStrategy: Send + Sync + Debug {
    fn group(&self, statements: Vec<Statement>) -> Vec<Vec<Statement>>;
}

/// The whole file is one unit (ETL procedures, delta-firewall views, PML scripts).
#[derive(Debug, Clone, Copy, Default)]
pub struct PerFileGrouping;

impl GroupingStrategy for PerFileGrouping {
    fn group(&self, statements: Vec<Statement>) -> Vec<Vec<Statement>> {
        if statements.is_empty() {
            Vec::new()
        } else {
            vec![statements]
        }
    }
}

/// One unit per statement, except that a `DROP x` is joined with a later `CREATE x` of
/// the same name, as `VaultSpeed` emits DROP and CREATE sections for the same tables.
/// The pair takes the CREATE's position. Unpaired drops stay standalone units so the
/// planner can decide about them explicitly.
#[derive(Debug, Clone, Copy, Default)]
pub struct DropCreatePairing;

impl GroupingStrategy for DropCreatePairing {
    fn group(&self, statements: Vec<Statement>) -> Vec<Vec<Statement>> {
        let mut slots: Vec<Vec<Statement>> = Vec::with_capacity(statements.len());
        let mut open_drops: HashMap<String, usize> = HashMap::new();

        for statement in statements {
            let name = statement
                .target
                .as_ref()
                .map(vdp_vaultspeed_domain::ObjectRef::qualified_name);
            match (&statement.kind, name) {
                (kind, Some(name)) if kind.is_drop() => {
                    open_drops.insert(name, slots.len());
                    slots.push(vec![statement]);
                }
                (kind, Some(name)) if kind.is_create() => match open_drops.remove(&name) {
                    Some(slot) => {
                        let mut pair = std::mem::take(&mut slots[slot]);
                        pair.push(statement);
                        slots.push(pair);
                    }
                    None => slots.push(vec![statement]),
                },
                _ => slots.push(vec![statement]),
            }
        }
        slots.into_iter().filter(|slot| !slot.is_empty()).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vdp_vaultspeed_sql::{ArtifactPipeline, NoRewrite, RoutineAwareSplitter};

    use super::*;

    fn statements(sql: &str) -> Vec<Statement> {
        ArtifactPipeline::new(Arc::new(RoutineAwareSplitter), vec![], Arc::new(NoRewrite))
            .statements(sql)
            .unwrap()
    }

    fn shape(groups: &[Vec<Statement>]) -> Vec<Vec<String>> {
        groups
            .iter()
            .map(|g| g.iter().map(|s| s.kind.label()).collect())
            .collect()
    }

    #[test]
    fn pairs_drops_with_later_creates() {
        let groups = DropCreatePairing.group(statements(
            "DROP TABLE IF EXISTS s.a; DROP TABLE IF EXISTS s.b; DROP VIEW IF EXISTS s.v;\
             CREATE TABLE s.a (x INT); ALTER TABLE s.a COMMENT 'c'; CREATE TABLE s.b (x INT);",
        ));
        assert_eq!(
            shape(&groups),
            vec![
                vec!["DROP VIEW"],
                vec!["DROP TABLE", "CREATE TABLE"],
                vec!["ALTER TABLE COMMENT"],
                vec!["DROP TABLE", "CREATE TABLE"],
            ]
        );
    }

    #[test]
    fn per_file_keeps_everything_together() {
        let groups = PerFileGrouping.group(statements(
            "DROP VIEW IF EXISTS v; CREATE VIEW v AS SELECT 1;",
        ));
        assert_eq!(shape(&groups), vec![vec!["DROP VIEW", "CREATE VIEW"]]);
        assert_eq!(
            PerFileGrouping.group(Vec::new()),
            Vec::<Vec<Statement>>::new()
        );
    }
}
