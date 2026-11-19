//! `dbt_project.yml`, as far as the toolkit needs it.

use std::path::{Path, PathBuf};

use dbt_yaml::Value;

use crate::DbtError;

/// The parts of `dbt_project.yml` the modules use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbtProject {
    pub root: PathBuf,
    pub name: String,
    /// The `profile:` key (the Python utilities wrongly used `name`).
    pub profile: String,
    pub model_paths: Vec<PathBuf>,
    pub seed_paths: Vec<PathBuf>,
    pub macro_paths: Vec<PathBuf>,
    pub target_path: PathBuf,
}

impl DbtProject {
    /// Reads the `dbt_project.yml` of the project at `root`.
    ///
    /// # Errors
    ///
    /// [`DbtError::Project`] when the file cannot be read, is not valid YAML or has no `name`.
    pub fn load(root: &Path) -> Result<Self, DbtError> {
        let path = root.join("dbt_project.yml");
        let text = std::fs::read_to_string(&path).map_err(|e| DbtError::Project {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        Self::parse(root, &text).map_err(|detail| DbtError::Project { path, detail })
    }

    fn parse(root: &Path, text: &str) -> Result<Self, String> {
        let doc: Value = dbt_yaml::from_str(text).map_err(|e| e.to_string())?;
        let string = |key: &str| doc.get(key).and_then(Value::as_str).map(str::to_owned);
        let name = string("name").ok_or("no `name`")?;
        let profile = string("profile").unwrap_or_else(|| name.clone());
        // All `*-paths`, not just the last one (the Python utilities popped the last entry).
        let paths = |key: &str, default: &str| -> Vec<PathBuf> {
            match doc.get(key).and_then(Value::as_sequence) {
                Some(items) => items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|p| root.join(p))
                    .collect(),
                None => vec![root.join(default)],
            }
        };
        Ok(Self {
            root: root.to_owned(),
            model_paths: paths("model-paths", "models"),
            seed_paths: paths("seed-paths", "seeds"),
            macro_paths: paths("macro-paths", "macros"),
            target_path: root.join(string("target-path").unwrap_or_else(|| "target".into())),
            name,
            profile,
        })
    }

    /// The `manifest.json` in the project's target path.
    pub fn manifest_path(&self) -> PathBuf {
        self.target_path.join("manifest.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_profile_and_all_paths() {
        let project = DbtProject::parse(
            Path::new("/p"),
            "name: x\nprofile: y\nmodel-paths: [models, extra]\n",
        )
        .unwrap();
        assert_eq!(project.profile, "y");
        assert_eq!(
            project.model_paths,
            [PathBuf::from("/p/models"), PathBuf::from("/p/extra")]
        );
        assert_eq!(project.seed_paths, [PathBuf::from("/p/seeds")]);
        assert_eq!(
            project.manifest_path(),
            PathBuf::from("/p/target/manifest.json")
        );
    }
}
