use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::DbtError;

/// The dbt engine: `explicit` (config), else `$VDP_DBT_BIN`, else the first `dbt` on `PATH` that is not the
/// running wrapper (so `/usr/local/bin/dbt -> vdp` never calls itself).
///
/// # Errors
///
/// [`DbtError::NotFound`] when no engine is configured, `$VDP_DBT_BIN` is unset or empty and `PATH` holds no `dbt`
/// other than the running wrapper. A configured path is returned as given, without checking that it exists.
pub fn locate_engine(explicit: Option<&Path>) -> Result<PathBuf, DbtError> {
    let env = std::env::var_os("VDP_DBT_BIN").filter(|v| !v.is_empty());
    let path = std::env::var_os("PATH").unwrap_or_default();
    let me = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::canonicalize(p).ok());
    locate_in(explicit, env, &path, me.as_deref())
}

fn locate_in(
    explicit: Option<&Path>,
    env: Option<OsString>,
    path: &std::ffi::OsStr,
    me: Option<&Path>,
) -> Result<PathBuf, DbtError> {
    if let Some(binary) = explicit.filter(|p| !p.as_os_str().is_empty()) {
        return Ok(binary.to_owned());
    }
    if let Some(binary) = env {
        return Ok(PathBuf::from(binary));
    }
    std::env::split_paths(path)
        .map(|dir| dir.join("dbt"))
        .filter(|candidate| candidate.is_file())
        .find(|candidate| {
            let canonical = std::fs::canonicalize(candidate).ok();
            canonical.as_deref() != me
        })
        .ok_or(DbtError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_itself_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let (first, second) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("dbt"), "wrapper").unwrap();
        std::fs::write(second.join("dbt"), "engine").unwrap();
        let me = std::fs::canonicalize(first.join("dbt")).unwrap();
        let path = std::env::join_paths([&first, &second]).unwrap();
        let found = locate_in(None, None, &path, Some(&me)).unwrap();
        assert_eq!(found, second.join("dbt"));
        let explicit = locate_in(Some(Path::new("/x/dbt")), None, &path, Some(&me)).unwrap();
        assert_eq!(explicit, PathBuf::from("/x/dbt"));
        assert!(locate_in(None, None, &OsString::new(), None).is_err());
    }
}
