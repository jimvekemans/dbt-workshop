use std::path::{Path, PathBuf};

use crate::CoreError;

/// A dbt project on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    /// The directory holding `dbt_project.yml`.
    pub root: PathBuf,
    /// The enclosing git work tree, when there is one.
    pub repo_root: Option<PathBuf>,
}

impl ProjectLayout {
    /// The file that marks a dbt project root.
    pub const PROJECT_FILE: &'static str = "dbt_project.yml";

    /// Walks up from `start` to the nearest directory with a `dbt_project.yml`. Running from `models/` or any
    /// other sub-folder therefore works (the Python utilities used the current directory as the project).
    ///
    /// # Errors
    ///
    /// [`CoreError::NoProject`] when neither `start` nor any parent directory holds a `dbt_project.yml`.
    pub fn discover(start: &Path) -> Result<Self, CoreError> {
        let start = absolute(start);
        let root = start
            .ancestors()
            .find(|dir| dir.join(Self::PROJECT_FILE).is_file())
            .ok_or_else(|| CoreError::NoProject(start.clone()))?
            .to_owned();
        Ok(Self::at(&root))
    }

    /// A project rooted exactly at `root` (`--project-dir`).
    pub fn at(root: &Path) -> Self {
        let root = absolute(root);
        let repo_root = root
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .map(Path::to_owned);
        Self { root, repo_root }
    }

    /// The path of the project's `dbt_project.yml`.
    pub fn project_file(&self) -> PathBuf {
        self.root.join(Self::PROJECT_FILE)
    }

    /// `<root>/<relative>` unless `relative` is absolute.
    pub fn resolve(&self, relative: &Path) -> PathBuf {
        if relative.is_absolute() {
            relative.to_owned()
        } else {
            self.root.join(relative)
        }
    }
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_the_project_from_a_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("repo/project");
        std::fs::create_dir_all(project.join("models/staging")).unwrap();
        std::fs::create_dir_all(dir.path().join("repo/.git")).unwrap();
        std::fs::write(project.join("dbt_project.yml"), "name: x\n").unwrap();

        let layout = ProjectLayout::discover(&project.join("models/staging")).unwrap();
        assert_eq!(layout.root, std::path::absolute(&project).unwrap());
        assert_eq!(
            layout.repo_root,
            Some(std::path::absolute(dir.path().join("repo")).unwrap())
        );
        assert!(ProjectLayout::discover(dir.path()).is_err());
    }
}
