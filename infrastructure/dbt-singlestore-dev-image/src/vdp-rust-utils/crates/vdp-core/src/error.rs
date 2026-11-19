use std::path::PathBuf;

use thiserror::Error;

/// Why configuration, the project or a service could not be loaded.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid configuration in {origin}: {detail}")]
    Config { origin: String, detail: String },

    /// `known`: the configured environments (`[environments.<name>]`).
    #[error("unknown environment '{name}' (configured: {}; add [environments.{name}] to define it)", .known.join(", "))]
    UnknownEnvironment { name: String, known: Vec<String> },

    #[error("no dbt project (dbt_project.yml) found in {0} or any parent directory")]
    NoProject(PathBuf),

    #[error(transparent)]
    Service(#[from] crate::ServiceError),
}

/// Renders `error` and its [`std::error::Error::source`] chain joined with `": "`,
/// skipping any source whose text the accumulated message already ends with.
pub fn display_chain(error: &dyn std::error::Error) -> String {
    use std::fmt::Write as _;
    let mut out = error.to_string();
    let mut current = error.source();
    while let Some(source) = current {
        let next = source.to_string();
        if !next.is_empty() && !out.ends_with(&next) {
            let _ = write!(out, ": {next}");
        }
        current = source.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Error)]
    #[error("{0}")]
    struct Leaf(&'static str);

    #[derive(Debug, Error)]
    #[error("{label}")]
    struct Mid {
        label: &'static str,
        #[source]
        source: Leaf,
    }

    #[derive(Debug, Error)]
    #[error("{label}")]
    struct Outer {
        label: &'static str,
        #[source]
        source: Box<dyn std::error::Error>,
    }

    #[derive(Debug, Error)]
    #[error("i/o error: {0}")]
    struct Embedding(#[source] Leaf);

    #[test]
    fn display_chain_joins_three_levels_and_keeps_leaf() {
        let leaf = Leaf("Permission denied (os error 13)");
        assert_eq!(display_chain(&leaf), "Permission denied (os error 13)");

        let deep = Outer {
            label: "source discovery failed",
            source: Box::new(Mid {
                label: "/p",
                source: Leaf("Permission denied (os error 13)"),
            }),
        };
        assert_eq!(
            display_chain(&deep),
            "source discovery failed: /p: Permission denied (os error 13)"
        );
    }

    #[test]
    fn display_chain_dedupes_source_already_embedded_in_parent_display() {
        let embedded = Outer {
            label: "a.zip is not a readable zip archive",
            source: Box::new(Embedding(Leaf("Input/output error (os error 5)"))),
        };
        assert_eq!(
            display_chain(&embedded),
            "a.zip is not a readable zip archive: i/o error: Input/output error (os error 5)"
        );
    }
}
