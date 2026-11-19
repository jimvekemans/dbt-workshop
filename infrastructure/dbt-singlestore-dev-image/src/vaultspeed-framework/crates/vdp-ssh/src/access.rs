use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use vdp_secrets::{ExposeSecret, SecretProvider, first_of};

use crate::{SshEndpoint, SshError};

/// Which secrets describe the remote host, and the defaults when they are absent.
#[derive(Debug, Clone, Default)]
pub struct SecretNames {
    /// The host (first one set wins; a scheme and a `:port` are allowed).
    pub host: Vec<String>,
    /// The user; empty or unset: `default_user`.
    pub user: Vec<String>,
    /// The private key (PEM); unset: the SSH agent or the user's own keys.
    pub key: Vec<String>,
    pub default_user: Option<String>,
    pub default_port: u16,
    /// `known_hosts`; `None`: the user's own. Host keys are always checked.
    pub known_hosts: Option<PathBuf>,
}

/// An endpoint built from secrets, plus the private key file it uses. The key file is private (0600) and is
/// deleted when this is dropped, so keep it alive as long as the endpoint is used.
#[derive(Debug)]
pub struct SshAccess {
    pub endpoint: SshEndpoint,
    pub key_file: Option<tempfile::NamedTempFile>,
}

fn names(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

impl SshAccess {
    /// The endpoint for `names`, or `None` when no host secret is set.
    ///
    /// # Errors
    ///
    /// [`SshError::MissingSecret`] when a host is set but neither a user secret nor a default user is; [`SshError::Io`] when
    /// the private key file cannot be created or written.
    pub fn from_secrets(
        secrets: &dyn SecretProvider,
        names_: &SecretNames,
    ) -> Result<Option<Self>, SshError> {
        let Some(host) = first_of(secrets, &names(&names_.host)) else {
            return Ok(None);
        };
        let user = first_of(secrets, &names(&names_.user))
            .map(|u| u.expose_secret().to_owned())
            .or_else(|| names_.default_user.clone())
            .ok_or_else(|| SshError::MissingSecret(names_.user.join(" or ")))?;
        let host = host.expose_secret();
        let host = host
            .split_once("://")
            .map_or(host, |(_, rest)| rest)
            .trim_end_matches('/');
        let (host, port) = match host.rsplit_once(':') {
            Some((h, p)) if p.parse::<u16>().is_ok() => {
                (h, p.parse().unwrap_or(names_.default_port))
            }
            _ => (host, names_.default_port),
        };
        let key_file = match first_of(secrets, &names(&names_.key)) {
            Some(key) => {
                let key_error = |source| SshError::Io {
                    path: std::env::temp_dir(),
                    source,
                };
                let mut file = tempfile::NamedTempFile::new().map_err(key_error)?;
                file.as_file()
                    .set_permissions(std::fs::Permissions::from_mode(0o600))
                    .map_err(key_error)?;
                let mut pem = key.expose_secret().trim().to_owned();
                pem.push('\n');
                file.write_all(pem.as_bytes()).map_err(key_error)?;
                Some(file)
            }
            None => None,
        };
        Ok(Some(Self {
            endpoint: SshEndpoint {
                user,
                host: host.to_owned(),
                port,
                identity_file: key_file.as_ref().map(|k| k.path().to_owned()),
                known_hosts: names_.known_hosts.clone(),
            },
            key_file,
        }))
    }
}

#[cfg(test)]
mod tests {
    use vdp_secrets::MapProvider;

    use super::*;

    fn names() -> SecretNames {
        SecretNames {
            host: vec!["AIRFLOW_URL".into(), "AIRFLOW_HOST".into()],
            user: vec!["username".into()],
            key: vec!["private_key".into()],
            default_user: Some("airflow".into()),
            default_port: 22,
            known_hosts: None,
        }
    }

    #[test]
    fn host_scheme_port_user_and_key() {
        let secrets = MapProvider::new("test")
            .with("AIRFLOW_URL", "https://airflow.example:2222/")
            .with(
                "private_key",
                "-----BEGIN KEY-----\nabc\n-----END KEY-----\n",
            );
        let access = SshAccess::from_secrets(&secrets, &names())
            .unwrap()
            .unwrap();
        assert_eq!(access.endpoint.host, "airflow.example");
        assert_eq!(access.endpoint.port, 2222);
        assert_eq!(access.endpoint.user, "airflow");
        let key = access.key_file.as_ref().unwrap().path();
        assert_eq!(
            std::fs::metadata(key).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(std::fs::read_to_string(key).unwrap().ends_with("-----\n"));
        assert_eq!(access.endpoint.identity_file.as_deref(), Some(key));
    }

    #[test]
    fn no_host_means_no_access_and_a_user_is_needed() {
        assert!(
            SshAccess::from_secrets(&MapProvider::new("test").with("x", "y"), &names())
                .unwrap()
                .is_none()
        );
        let mut strict = names();
        strict.default_user = None;
        assert!(matches!(
            SshAccess::from_secrets(&MapProvider::new("test").with("AIRFLOW_HOST", "h"), &strict),
            Err(SshError::MissingSecret(_))
        ));
    }
}
