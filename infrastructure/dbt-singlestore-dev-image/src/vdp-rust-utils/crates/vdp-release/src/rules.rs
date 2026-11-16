//! Which branches the release flows may write.
//!
//! `dev` (the source) is the tested dev environment: no flow may ever commit on it, not even locally, so a
//! revert can never reach it. Releases and patches write `<prefix>*` branches; only a promotion writes main.

use vdp_git::WriteGuard;

use crate::{ReleaseError, ReleaseSettings, local_name};

/// The branch names a flow may write, from `[release]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchRules {
    /// `release/`.
    pub prefix: String,
    /// The source branch, local name (`dev`): never written.
    pub source: String,
    /// The production branch, local name (`main`): written by promotions only.
    pub main: String,
    /// Jira key prefix in commit messages (`INTEL`).
    pub ticket_prefix: String,
}

impl BranchRules {
    pub fn from_settings(settings: &ReleaseSettings) -> Self {
        Self {
            prefix: settings.branch_prefix.clone(),
            source: local_name(&settings.from),
            main: local_name(&settings.to),
            ticket_prefix: settings.ticket_prefix.clone(),
        }
    }

    /// The local name of a release branch the flows may create, patch or promote; anything else (`dev`,
    /// `main`, `feature/x`, an invalid ref name) is refused.
    pub fn release_target(&self, name: &str) -> Result<String, ReleaseError> {
        let branch = local_name(name);
        if branch == self.source || branch == self.main {
            return Err(ReleaseError::Invalid(format!(
                "{branch} is not a release branch: the release tools never write {}",
                if branch == self.source {
                    "the source branch"
                } else {
                    "main except by promotion"
                }
            )));
        }
        let suffix = branch.strip_prefix(&self.prefix).unwrap_or_default();
        if suffix.is_empty() {
            return Err(ReleaseError::Invalid(format!(
                "{branch} is not a release branch: it must start with {}",
                self.prefix
            )));
        }
        if !valid_ref_name(&branch) {
            return Err(ReleaseError::Invalid(format!(
                "{branch} is not a valid branch name"
            )));
        }
        Ok(branch)
    }

    /// The git-layer guard: the source branch is protected; release branches and main are the only writable
    /// branches.
    pub fn guard(&self) -> WriteGuard {
        WriteGuard {
            protected: vec![self.source.clone()],
            writable: vec![self.prefix.clone(), self.main.clone()],
        }
    }
}

/// `git check-ref-format` rules for a branch name.
#[allow(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "git's rule is case-sensitive: only `.lock` is refused"
)]
fn valid_ref_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.ends_with('.')
        && !name.contains("..")
        && !name.contains("//")
        && !name.contains("@{")
        && name != "@"
        && !name
            .chars()
            .any(|c| c.is_ascii_control() || c.is_whitespace() || "~^:?*[\\".contains(c))
        && name
            .split('/')
            .all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> BranchRules {
        BranchRules::from_settings(&ReleaseSettings::default())
    }

    #[test]
    fn only_release_branches_are_targets() {
        let rules = rules();
        assert_eq!(rules.source, "dev");
        assert_eq!(rules.main, "main");
        assert_eq!(
            rules.release_target("origin/release/2026_DROP25").unwrap(),
            "release/2026_DROP25"
        );
        for name in [
            "dev",
            "origin/dev",
            "main",
            "origin/main",
            "feature/x",
            "release/",
            "release/a b",
            "release/x..y",
            "release/.hidden",
            "release/x.lock",
        ] {
            assert!(rules.release_target(name).is_err(), "{name}");
        }
    }

    #[test]
    fn the_guard_protects_dev() {
        let guard = rules().guard();
        assert!(guard.check("dev").is_err());
        assert!(guard.check("origin/dev").is_err());
        assert!(guard.check("feature/x").is_err());
        assert!(guard.check("release/").is_err());
        assert!(guard.check("release/2026_DROP25").is_ok());
        assert!(guard.check("main").is_ok());
    }
}
