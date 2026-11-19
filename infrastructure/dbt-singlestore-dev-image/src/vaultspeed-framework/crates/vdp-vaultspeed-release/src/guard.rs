use std::collections::BTreeMap;

use vdp_git::Vcs;

use crate::{VsError, VsRules};

/// Commits each environment made itself: the first-parent history of its branch beyond `main` (merges into it,
/// CI's deploy commits, direct fixes). They never leave that environment.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentCommits {
    owner: BTreeMap<String, String>,
}

/// Arguments for [`EnvironmentCommits::check`].
#[derive(Debug, Clone, Copy)]
pub struct CommitCheck<'a> {
    pub rules: &'a VsRules,
    pub rev: &'a str,
    pub what: &'a str,
}

impl EnvironmentCommits {
    /// Finds which environment owns each commit that is not on `main`.
    ///
    /// # Errors
    ///
    /// [`VsError::Git`] when the remote branches or their history cannot be read.
    pub fn load(vcs: &dyn Vcs, rules: &VsRules) -> Result<Self, VsError> {
        let main = format!("origin/{}", rules.settings.main);
        let mut owner = BTreeMap::new();
        for env in rules.environments() {
            if vcs.remote_head(env)?.is_none() {
                continue;
            }
            for commit in vcs.log(&main, &format!("origin/{env}"), true)? {
                owner.insert(commit.hash, env.clone());
            }
        }
        Ok(Self { owner })
    }

    /// `Err` when `check.rev` contains a commit of an environment: environment content never reaches `main`, a
    /// release branch or another environment, and nothing may be based on an environment branch.
    ///
    /// # Errors
    ///
    /// [`VsError::Refused`] when `check.rev` contains a commit of an environment; [`VsError::Git`] when the history cannot be
    /// read.
    pub fn check(&self, vcs: &dyn Vcs, check: CommitCheck<'_>) -> Result<(), VsError> {
        let main = format!("origin/{}", check.rules.settings.main);
        for commit in vcs.log(&main, check.rev, false)? {
            if let Some(env) = self.owner.get(&commit.hash) {
                return Err(VsError::Refused(format!(
                    "{} contains {} ({}) of the {env} branch: environment content never leaves its \
                     environment, and nothing is based on an environment branch",
                    check.what,
                    commit.short(),
                    commit.subject()
                )));
            }
        }
        Ok(())
    }
}
