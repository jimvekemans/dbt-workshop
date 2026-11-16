use std::collections::BTreeMap;

use vdp_git::Vcs;

use crate::{VsError, VsRules};

/// Commits each environment made itself: the first-parent history of its branch beyond `main` (merges into it,
/// CI's deploy commits, direct fixes). They never leave that environment.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentCommits {
    owner: BTreeMap<String, String>,
}

impl EnvironmentCommits {
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

    /// `Err` when `rev` contains a commit of an environment: environment content never reaches `main`, a
    /// release branch or another environment, and nothing may be based on an environment branch.
    pub fn check(
        &self,
        vcs: &dyn Vcs,
        rules: &VsRules,
        rev: &str,
        what: &str,
    ) -> Result<(), VsError> {
        let main = format!("origin/{}", rules.settings.main);
        for commit in vcs.log(&main, rev, false)? {
            if let Some(env) = self.owner.get(&commit.hash) {
                return Err(VsError::Refused(format!(
                    "{what} contains {} ({}) of the {env} branch: environment content never leaves its \
                     environment, and nothing is based on an environment branch",
                    commit.short(),
                    commit.subject()
                )));
            }
        }
        Ok(())
    }
}
