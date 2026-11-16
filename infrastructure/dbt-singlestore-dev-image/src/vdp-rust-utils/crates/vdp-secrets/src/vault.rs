use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use ureq::Agent;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::{SecretError, SecretProvider};

/// Minimal `HashiCorp` Vault client: `AppRole` login and KV v2 reads.
///
/// TLS is verified, against `ca_certificate` when given (the Python client used
/// `verify=False`). `HTTPS_PROXY` is honoured, as in CI.
pub struct VaultClient {
    url: String,
    agent: Agent,
    token: SecretString,
}

impl VaultClient {
    pub fn login_approle(
        url: &str,
        role_id: &SecretString,
        secret_id: &SecretString,
        ca_certificate: Option<&Path>,
    ) -> Result<Self, SecretError> {
        let url = url.trim_end_matches('/').to_owned();
        let agent = build_agent(&url, ca_certificate)?;
        let login_url = format!("{url}/v1/auth/approle/login");
        let response: Value = agent
            .post(&login_url)
            .send_json(json!({
                "role_id": role_id.expose_secret(),
                "secret_id": secret_id.expose_secret(),
            }))
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| vault_error(&login_url, &e))?;
        let token =
            response["auth"]["client_token"]
                .as_str()
                .ok_or_else(|| SecretError::Vault {
                    url: login_url.clone(),
                    detail: "response has no auth.client_token".into(),
                })?;
        tracing::info!(%url, "logged in to Vault");
        Ok(Self {
            url,
            agent,
            token: SecretString::from(token.to_owned()),
        })
    }

    /// Reads all string values of a KV v2 secret.
    pub fn read_kv2(
        &self,
        mount: &str,
        path: &str,
    ) -> Result<HashMap<String, SecretString>, SecretError> {
        let url = format!("{}/v1/{mount}/data/{}", self.url, path.trim_matches('/'));
        let response: Value = self
            .agent
            .get(&url)
            .header("X-Vault-Token", self.token.expose_secret())
            .call()
            .and_then(|mut r| r.body_mut().read_json())
            .map_err(|e| vault_error(&url, &e))?;
        let data = response["data"]["data"]
            .as_object()
            .ok_or_else(|| SecretError::Vault {
                url: url.clone(),
                detail: "response has no data.data object".into(),
            })?;
        Ok(data
            .iter()
            .filter_map(|(k, v)| {
                v.as_str()
                    .map(|s| (k.clone(), SecretString::from(s.to_owned())))
            })
            .collect())
    }

    /// Revokes the token; failures are logged only.
    pub fn revoke(self) {
        let url = format!("{}/v1/auth/token/revoke-self", self.url);
        if let Err(error) = self
            .agent
            .post(&url)
            .header("X-Vault-Token", self.token.expose_secret())
            .send_empty()
        {
            tracing::warn!(%error, "could not revoke the Vault token");
        }
    }
}

impl fmt::Debug for VaultClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultClient")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

fn build_agent(url: &str, ca_certificate: Option<&Path>) -> Result<Agent, SecretError> {
    let mut tls = TlsConfig::builder();
    if let Some(path) = ca_certificate {
        let pem = std::fs::read(path).map_err(|e| SecretError::File {
            path: path.to_owned(),
            detail: e.to_string(),
        })?;
        let certificate = Certificate::from_pem(&pem).map_err(|e| SecretError::Vault {
            url: url.to_owned(),
            detail: format!("invalid CA certificate {}: {e}", path.display()),
        })?;
        tls = tls.root_certs(RootCerts::new_with_certs(&[certificate.to_owned()]));
    }
    Ok(Agent::config_builder()
        .tls_config(tls.build())
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into())
}

fn vault_error(url: &str, error: &ureq::Error) -> SecretError {
    SecretError::Vault {
        url: url.to_owned(),
        detail: error.to_string(),
    }
}

/// Secrets read once from one or more KV v2 paths; later paths do not override earlier ones.
pub struct VaultKvProvider {
    label: String,
    values: HashMap<String, SecretString>,
}

impl VaultKvProvider {
    pub fn load(client: &VaultClient, mount: &str, paths: &[String]) -> Result<Self, SecretError> {
        let mut values = HashMap::new();
        for path in paths {
            for (key, value) in client.read_kv2(mount, path)? {
                values.entry(key).or_insert(value);
            }
        }
        Ok(Self {
            label: format!("vault {mount}:{}", paths.join(",")),
            values,
        })
    }
}

impl fmt::Debug for VaultKvProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VaultKvProvider")
            .field("label", &self.label)
            .field("keys", &self.values.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl SecretProvider for VaultKvProvider {
    fn name(&self) -> String {
        self.label.clone()
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        self.values.get(key).cloned()
    }

    fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.values.keys().cloned().collect();
        keys.sort();
        keys
    }
}
