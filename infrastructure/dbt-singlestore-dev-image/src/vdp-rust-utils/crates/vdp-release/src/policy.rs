use serde::{Deserialize, Serialize};
use vdp_jira::{FixVersion, Ticket};

/// What the policy says about one ticket, before anyone overrides it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum Classification {
    /// In the release.
    Keep(String),
    /// Out of the release.
    Revert(String),
    /// Jira has no DROP fix version (or Jira could not be asked).
    NoFixVersion,
}

impl Classification {
    pub const fn is_keep(&self) -> bool {
        matches!(self, Self::Keep(_))
    }

    pub const fn is_revert(&self) -> bool {
        matches!(self, Self::Revert(_))
    }
}

/// Why a ticket stays in the release or is reverted (the CLI's two-way view of a [`Classification`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Keep(String),
    Revert(String),
}

impl Decision {
    pub const fn reverts(&self) -> bool {
        matches!(self, Self::Revert(_))
    }
}

/// The release rules of the release managers.
///
/// - A ticket targeting an emergency drop (`<year>_EDROP<n>`, any number: EDROPs are numbered independently of
///   the DROPs) is always included, in releases and in patches.
/// - Otherwise its latest DROP decides: after the target, it is reverted.
/// - Without a DROP, an excluded fix version ("Nog in te plannen") reverts it; nothing at all is
///   [`Classification::NoFixVersion`].
#[derive(Debug, Clone)]
pub struct ReleasePolicy {
    pub target: FixVersion,
    /// The CLI's `--keep-empty-fixversions`: [`Classification::NoFixVersion`] decides Keep instead of Revert.
    pub keep_empty: bool,
    pub excluded: Vec<String>,
}

impl ReleasePolicy {
    pub fn classify(&self, ticket: &Ticket) -> Classification {
        let emergency = ticket.emergency_drops();
        if !emergency.is_empty() {
            let names: Vec<&str> = emergency.iter().map(FixVersion::name).collect();
            return Classification::Keep(format!(
                "{} (emergency drop: always included)",
                names.join(", ")
            ));
        }
        if let Some(version) = ticket.release() {
            return if version > self.target {
                Classification::Revert(format!("{version} is after {}", self.target))
            } else {
                Classification::Keep(format!("{version}"))
            };
        }
        if let Some(name) = ticket
            .fix_versions
            .iter()
            .find(|n| self.excluded.contains(n))
        {
            return Classification::Revert(format!("fix version '{name}' is excluded"));
        }
        Classification::NoFixVersion
    }

    /// The CLI's decision: [`Classification::NoFixVersion`] follows `keep_empty`.
    pub fn decide(&self, ticket: &Ticket) -> Decision {
        match self.classify(ticket) {
            Classification::Keep(reason) => Decision::Keep(reason),
            Classification::Revert(reason) => Decision::Revert(reason),
            Classification::NoFixVersion if self.keep_empty => {
                Decision::Keep("no DROP fix version (kept: --keep-empty-fixversions)".into())
            }
            Classification::NoFixVersion => Decision::Revert("no DROP fix version".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(keep_empty: bool) -> ReleasePolicy {
        ReleasePolicy {
            target: "2026_DROP05".parse().unwrap(),
            keep_empty,
            excluded: vec!["Nog in te plannen".into()],
        }
    }

    fn ticket(versions: &[&str]) -> Ticket {
        Ticket {
            key: "INTEL-1".into(),
            status: "Done".into(),
            fix_versions: versions.iter().map(|v| (*v).to_owned()).collect(),
            ..Ticket::default()
        }
    }

    /// The Python test matrix (test_release_manager.py), plus the cases it got wrong.
    #[test]
    fn python_matrix() {
        assert!(policy(false).decide(&ticket(&[])).reverts());
        assert!(!policy(true).decide(&ticket(&[])).reverts());
        assert!(
            policy(false)
                .decide(&ticket(&["Nog in te plannen"]))
                .reverts()
        );
        assert!(policy(false).decide(&ticket(&["2026_DROP06"])).reverts());
        assert!(policy(false).decide(&ticket(&["2027_DROP01"])).reverts());
        assert!(!policy(false).decide(&ticket(&["2026_DROP05"])).reverts());
        assert!(!policy(false).decide(&ticket(&["2026_DROP04"])).reverts());
        assert!(!policy(false).decide(&ticket(&["2025_DROP01"])).reverts());
    }

    #[test]
    fn fixes_over_python() {
        // "Nog in te plannen" never matched Python's DROP pattern, so --keep-empty-fixversions kept it.
        assert!(
            policy(true)
                .decide(&ticket(&["Nog in te plannen"]))
                .reverts()
        );
        // String comparison put DROP5 after DROP10.
        let late = ReleasePolicy {
            target: "2026_DROP10".parse().unwrap(),
            ..policy(false)
        };
        assert!(!late.decide(&ticket(&["2026_DROP5"])).reverts());
        // The latest DROP decides.
        assert!(
            policy(false)
                .decide(&ticket(&["2026_DROP04", "2026_DROP07"]))
                .reverts()
        );
    }

    #[test]
    fn emergency_drops_are_always_included() {
        // Any number: EDROPs count independently of the DROPs.
        for versions in [
            &["2026_EDROP09"][..],
            &["2026_EDROP01"],
            &["2027_EDROP02"],
            &["2026_DROP09", "2026_EDROP03"],
            &["Nog in te plannen", "2026_EDROP03"],
        ] {
            assert!(
                policy(false).classify(&ticket(versions)).is_keep(),
                "{versions:?}"
            );
        }
        assert_eq!(
            policy(false).classify(&ticket(&[])),
            Classification::NoFixVersion
        );
    }
}
