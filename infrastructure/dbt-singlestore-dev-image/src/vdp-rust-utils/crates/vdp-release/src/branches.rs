//! Release branches on origin.

use serde::{Deserialize, Serialize};
use vdp_git::Vcs;
use vdp_jira::FixVersion;

use crate::origin::BranchRole;
use crate::{ReleaseError, remote_ref};

/// A `release/*` branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ReleaseBranch {
    pub name: String,
    /// The suffix when it is a DROP fix version.
    pub fix_version: Option<String>,
    pub head: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub timestamp: i64,
    pub subject: String,
    /// Its head is contained in main.
    pub promoted: bool,
    /// Whether the release tools may change it (only the newest two).
    pub role: BranchRole,
}

/// `origin/<prefix>*`, newest first: by fix version when the suffix is one (numerically, so DROP10 after
/// DROP9), others after, by name. The Python version sorted names as strings.
pub fn release_branches(
    vcs: &dyn Vcs,
    prefix: &str,
    main: &str,
) -> Result<Vec<ReleaseBranch>, ReleaseError> {
    let main = remote_ref(main);
    let mut branches: Vec<(Option<FixVersion>, ReleaseBranch)> = Vec::new();
    for branch in vcs.remote_branches(prefix)? {
        let version: Option<FixVersion> = branch
            .name
            .strip_prefix(prefix)
            .and_then(|s| s.parse().ok());
        let promoted = vcs.is_ancestor(&branch.head, &main).unwrap_or(false);
        branches.push((
            version.clone(),
            ReleaseBranch {
                name: branch.name,
                fix_version: version.map(|v| v.name().to_owned()),
                head: branch.head,
                timestamp: branch.timestamp,
                subject: branch.subject,
                promoted,
                role: BranchRole::ReadOnly,
            },
        ));
    }
    branches.sort_by(|(va, a), (vb, b)| match (va, vb) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.name.cmp(&a.name),
    });
    let mut ranked = 0;
    Ok(branches
        .into_iter()
        .map(|(version, mut branch)| {
            if version.is_some() {
                branch.role = match ranked {
                    0 => BranchRole::Newest,
                    1 => BranchRole::Previous,
                    _ => BranchRole::ReadOnly,
                };
                ranked += 1;
            }
            branch
        })
        .collect())
}
