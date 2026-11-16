//! The dbt profile, so modules connect to exactly the database dbt uses.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dbt_yaml::Value;
use vdp_db::{ClientCertificate, ConnectionSettings, TlsMode};
use vdp_secrets::SecretString;

use crate::{DbtError, DbtProject};

/// Where a connection came from.
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub profiles_file: PathBuf,
    pub profile: String,
    pub target: String,
    pub settings: ConnectionSettings,
}

/// `profiles.yml`: `--profiles-dir`, else `$DBT_PROFILES_DIR`, else the project root, else `~/.dbt`.
pub fn profiles_file(project_root: &Path, profiles_dir: Option<&Path>) -> PathBuf {
    let candidates: Vec<PathBuf> = profiles_dir
        .map(Path::to_owned)
        .into_iter()
        .chain(std::env::var_os("DBT_PROFILES_DIR").map(PathBuf::from))
        .chain([project_root.to_owned()])
        .chain(std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".dbt")))
        .map(|dir| dir.join("profiles.yml"))
        .collect();
    candidates
        .iter()
        .find(|c| c.is_file())
        .unwrap_or(&candidates[0])
        .clone()
}

/// Reads the profile of `project` and maps its SingleStore target to connection settings.
///
/// `lookup` answers `env_var(...)`: the secrets dbt itself would see (dotenv, Vault, the environment).
pub fn resolve(
    project: &DbtProject,
    profiles_dir: Option<&Path>,
    target: Option<&str>,
    lookup: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedTarget, DbtError> {
    let file = profiles_file(&project.root, profiles_dir);
    let error = |detail: String| DbtError::Profile {
        path: file.clone(),
        detail,
    };
    let text = std::fs::read_to_string(&file).map_err(|e| error(e.to_string()))?;
    let names = names_in(&text);
    let values: BTreeMap<String, String> = names
        .into_iter()
        .filter_map(|name| lookup(&name).map(|v| (name, v)))
        .collect();
    let jinja = jinja(values);
    // As dbt does, only the values of the selected target are rendered: another target's `var('SCHEMA')` or
    // missing variable must not stop this one. A file whose Jinja is not inside YAML values (a `{% if %}` around
    // whole targets) is rendered as a whole first.
    let (doc, render_values): (Value, bool) = match dbt_yaml::from_str::<Value>(&text) {
        Ok(doc) => (doc, true),
        Err(_) => {
            let rendered = jinja
                .render_str(&text, ())
                .map_err(|e| error(e.to_string()))?;
            let doc = dbt_yaml::from_str(&rendered).map_err(|e| error(e.to_string()))?;
            (doc, false)
        }
    };
    let render = |value: String| -> Result<String, String> {
        if render_values && (value.contains("{{") || value.contains("{%")) {
            jinja.render_str(&value, ()).map_err(|e| e.to_string())
        } else {
            Ok(value)
        }
    };
    let profile = doc
        .get(project.profile.as_str())
        .ok_or_else(|| error(format!("no profile `{}`", project.profile)))?;
    let default_target = profile
        .get("target")
        .and_then(scalar)
        .map(&render)
        .transpose()
        .map_err(|e| error(format!("target: {e}")))?;
    let target_name = target
        .map(str::to_owned)
        .or(default_target)
        .ok_or_else(|| {
            error(format!(
                "profile `{}` has no default target",
                project.profile
            ))
        })?;
    let output = profile
        .get("outputs")
        .and_then(|o| o.get(target_name.as_str()))
        .ok_or_else(|| {
            error(format!(
                "profile `{}` has no target `{target_name}`",
                project.profile
            ))
        })?;
    let settings = to_settings(output, &render)
        .map_err(|detail| error(format!("target `{target_name}`: {detail}")))?;
    Ok(ResolvedTarget {
        profiles_file: file.clone(),
        profile: project.profile.clone(),
        target: target_name,
        settings,
    })
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s, ..) => Some(s.clone()),
        Value::Number(n, ..) => Some(n.to_string()),
        Value::Bool(b, ..) => Some(b.to_string()),
        _ => None,
    }
}

/// The Jinja of profiles.yml: `env_var('X', 'default')` from `values`, and `var('X', 'default')`, which gives the
/// default (dbt fills `var()` in a profile only from `--vars` on its command line, which the toolkit's own
/// connections do not have).
fn jinja(values: BTreeMap<String, String>) -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.add_function(
        "var",
        |name: String,
         default: Option<minijinja::Value>|
         -> Result<minijinja::Value, minijinja::Error> {
            default.ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("var('{name}') has no default (vdp does not see dbt's --vars)"),
                )
            })
        },
    );
    env.add_function(
        "env_var",
        move |name: String, default: Option<String>| -> Result<String, minijinja::Error> {
            values.get(&name).cloned().or(default).ok_or_else(|| {
                minijinja::Error::new(
                    minijinja::ErrorKind::InvalidOperation,
                    format!("env_var('{name}') is not set and has no default"),
                )
            })
        },
    );
    env
}

/// Variable names used with `env_var(...)`, so only those are looked up.
fn names_in(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("env_var(") {
        rest = &rest[start + "env_var(".len()..];
        let trimmed = rest.trim_start();
        if let Some(quote) = trimmed.chars().next().filter(|c| *c == '\'' || *c == '"') {
            let body = &trimmed[1..];
            if let Some(end) = body.find(quote) {
                names.push(body[..end].to_owned());
            }
        }
    }
    names
}

/// Maps a `type: singlestore` output with the rules of `/opt/dbt/crates/dbt-auth/src/singlestore/mod.rs`.
fn to_settings(
    output: &Value,
    render: &dyn Fn(String) -> Result<String, String>,
) -> Result<ConnectionSettings, String> {
    let lookup = |keys: &[&str]| -> Result<Option<String>, String> {
        keys.iter()
            .find_map(|k| output.get(*k).and_then(scalar).map(|v| ((*k), v)))
            .map(|(key, value)| render(value).map_err(|e| format!("{key}: {e}")))
            .transpose()
    };
    let kind = lookup(&["type"])?.unwrap_or_default();
    if kind != "singlestore" {
        return Err(format!("type is `{kind}`, expected `singlestore`"));
    }
    let host = lookup(&["host"])?.ok_or("no host")?;
    let port: u16 = lookup(&["port"])?
        .unwrap_or_else(|| "3306".into())
        .parse()
        .map_err(|_| "port is not a number")?;
    let user = lookup(&["user"])?.ok_or("no user")?;
    let mut settings = ConnectionSettings::new(host, port, user);
    settings.password = lookup(&["password", "token", "jwt"])?
        .filter(|p| !p.is_empty())
        .map(SecretString::from);
    settings.database = Some(lookup(&["database", "dbname"])?.ok_or("no database")?);
    let ca = lookup(&["ssl_ca", "sslrootcert"])?;
    let cert = lookup(&["ssl_cert"])?;
    let key = lookup(&["ssl_key"])?;
    if cert.is_some() != key.is_some() {
        return Err("both ssl_cert and ssl_key must be provided".into());
    }
    settings.tls = match lookup(&["ssl_mode", "sslmode"])? {
        Some(mode) => {
            TlsMode::parse(&mode).ok_or_else(|| format!("unsupported ssl_mode `{mode}`"))?
        }
        None if ca.is_some() || cert.is_some() => TlsMode::VerifyCa,
        None => TlsMode::Disabled,
    };
    settings.ca_certificate = ca.map(PathBuf::from);
    settings.client_certificate = cert.zip(key).map(|(certificate, key)| ClientCertificate {
        certificate: certificate.into(),
        key: key.into(),
    });
    if let Some(timeout) = lookup(&["connect_timeout", "timeout"])? {
        let digits = timeout.trim().trim_end_matches('s');
        if let Ok(seconds) = digits.parse::<u64>() {
            settings.connect_timeout = Duration::from_secs(seconds);
        }
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path, profiles: &str) -> DbtProject {
        std::fs::write(dir.join("dbt_project.yml"), "name: proj\nprofile: p\n").unwrap();
        std::fs::write(dir.join("profiles.yml"), profiles).unwrap();
        DbtProject::load(dir).unwrap()
    }

    const PROFILES: &str = "p:\n  target: dev\n  outputs:\n    dev:\n      type: singlestore\n      host: \"{{ env_var('SS_HOST', '127.0.0.1') }}\"\n      port: 3306\n      user: \"{{ env_var('SINGLESTORE_USER') }}\"\n      password: \"{{ env_var('SINGLESTORE_PASS') }}\"\n      database: db1\n      schema: db1\n    tls:\n      type: singlestore\n      host: h\n      user: u\n      database: d\n      ssl_ca: /ca.pem\n";

    #[test]
    fn renders_env_var_and_maps_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path(), PROFILES);
        let lookup = |name: &str| match name {
            "SINGLESTORE_USER" => Some("jdoe".to_owned()),
            "SINGLESTORE_PASS" => Some("pw".to_owned()),
            _ => None,
        };
        let resolved = resolve(&project, Some(dir.path()), None, &lookup).unwrap();
        assert_eq!(resolved.target, "dev");
        assert_eq!(resolved.settings.endpoint(), "jdoe@127.0.0.1:3306/db1");
        assert_eq!(resolved.settings.tls, TlsMode::Disabled);
        let tls = resolve(&project, Some(dir.path()), Some("tls"), &lookup).unwrap();
        // Certificates without ssl_mode mean verify_ca, as in dbt.
        assert_eq!(tls.settings.tls, TlsMode::VerifyCa);
        let missing = resolve(&project, Some(dir.path()), None, &|_| None).unwrap_err();
        assert!(
            missing.to_string().contains("SINGLESTORE_USER"),
            "{missing}"
        );
    }

    /// The client's profiles.yml: `var('SCHEMA', '')` in other targets, and an `env_var` without default only
    /// those targets need. Neither may stop the selected target; `var()` gives its default where it is used.
    #[test]
    fn only_the_selected_target_is_rendered_and_var_gives_its_default() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = "p:\n  target: \"{{ env_var('DBT_TARGET', 'default') }}\"\n  outputs:\n    default:\n      type: singlestore\n      host: h\n      user: u\n      database: \"{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_edw_dv_rdv\"\n    run:\n      type: singlestore\n      host: h\n      user: \"{{ env_var('RUN_USER') }}\"\n      database: d\n      schema: \"{{ var('SCHEMA', '') }}\"\n      threads: \"{{ var('THREADS', 4) }}\"\n    strict:\n      type: singlestore\n      host: h\n      user: u\n      database: \"{{ var('DATABASE') }}\"\n";
        let project = project(dir.path(), profiles);
        let none = |_: &str| None;
        let default = resolve(&project, Some(dir.path()), None, &none).unwrap();
        assert_eq!(default.target, "default");
        assert_eq!(default.settings.database.as_deref(), Some("dev_edw_dv_rdv"));
        let val = |name: &str| (name == "DBT_DEPLOY_TARGET").then(|| "val".to_owned());
        let default = resolve(&project, Some(dir.path()), None, &val).unwrap();
        assert_eq!(default.settings.database.as_deref(), Some("val_edw_dv_rdv"));

        let run = |name: &str| match name {
            "RUN_USER" => Some("runner".to_owned()),
            "DBT_TARGET" => Some("run".to_owned()),
            _ => None,
        };
        let selected = resolve(&project, Some(dir.path()), None, &run).unwrap();
        assert_eq!(
            selected.target, "run",
            "the default target name is rendered too"
        );
        assert_eq!(selected.settings.endpoint(), "runner@h:3306/d");
        // The run target needs RUN_USER: only when it is the one selected.
        let error = resolve(&project, Some(dir.path()), Some("run"), &none).unwrap_err();
        assert!(error.to_string().contains("RUN_USER"), "{error}");
        let error = resolve(&project, Some(dir.path()), Some("strict"), &none).unwrap_err();
        assert!(
            error.to_string().contains("var('DATABASE') has no default"),
            "{error}"
        );
    }

    /// Jinja around whole targets is not YAML: the file is rendered first, as before.
    #[test]
    fn jinja_outside_values_renders_the_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let profiles = "p:\n  target: dev\n  outputs:\n{% for t in ['dev', 'val'] %}\n    {{ t }}:\n      type: singlestore\n      host: h\n      user: u\n      database: \"{{ t }}_db\"\n{% endfor %}\n";
        let project = project(dir.path(), profiles);
        let val = resolve(&project, Some(dir.path()), Some("val"), &|_| None).unwrap();
        assert_eq!(val.settings.database.as_deref(), Some("val_db"));
    }
}
