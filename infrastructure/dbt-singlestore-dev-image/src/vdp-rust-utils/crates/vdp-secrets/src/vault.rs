use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use ureq::Agent;
use ureq::tls::{Certificate, RootCerts, TlsConfig};

use crate::{SecretError, SecretProvider};

/// Normalizes a Vault base URL by trimming whitespace and trailing slashes, and auto-prefixing
/// `https://` when no HTTP(S) scheme is present.
#[must_use]
pub fn normalize_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed
        .get(..8)
        .is_some_and(|s| s.eq_ignore_ascii_case("https://"))
    {
        format!("https://{}", &trimmed[8..])
    } else if trimmed
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("http://"))
    {
        format!("http://{}", &trimmed[7..])
    } else {
        format!("https://{trimmed}")
    }
}

const REVOKE_TIMEOUT: Duration = Duration::from_secs(3);

/// Minimal `HashiCorp` Vault client: `AppRole` login and KV v2 reads.
///
/// TLS is verified, against `ca_certificate` when given (the Python client used
/// `verify=False`). `HTTPS_PROXY` is honoured, as in CI.
///
/// Clients created via [`VaultClient::login_approle`] own their token and revoke it best-effort on
/// [`Drop`] (at most once; calling [`VaultClient::revoke`] explicitly also revokes once) with a
/// bounded timeout. Clients created with a pre-existing token via [`VaultClient::with_token`] never
/// revoke it.
pub struct VaultClient {
    url: String,
    agent: Agent,
    token: SecretString,
    revoke_on_drop: bool,
    revoke_timeout: Duration,
}

/// AppRole authentication inputs for [`VaultClient::login_approle`].
#[derive(Debug, Clone, Copy)]
pub struct AppRoleCredentials<'a> {
    pub url: &'a str,
    pub role_id: &'a SecretString,
    pub secret_id: &'a SecretString,
    pub ca_certificate: Option<&'a Path>,
}

impl VaultClient {
    /// Normalizes a Vault base URL using [`normalize_url`].
    #[must_use]
    pub fn normalize_url(url: &str) -> String {
        normalize_url(url)
    }

    /// Logs in with AppRole and keeps the resulting token, which is revoked when the client is dropped.
    ///
    /// # Errors
    ///
    /// [`SecretError::File`] when the CA certificate file cannot be read; [`SecretError::Vault`] when it is not valid PEM, the
    /// login request fails, or the response carries no `auth.client_token`.
    pub fn login_approle(credentials: &AppRoleCredentials<'_>) -> Result<Self, SecretError> {
        let url = normalize_url(credentials.url);
        let agent = build_agent(&url, credentials.ca_certificate)?;
        let login_url = format!("{url}/v1/auth/approle/login");
        let response: Value = agent
            .post(&login_url)
            .send_json(json!({
                "role_id": credentials.role_id.expose_secret(),
                "secret_id": credentials.secret_id.expose_secret(),
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
            revoke_on_drop: true,
            revoke_timeout: REVOKE_TIMEOUT,
        })
    }

    /// Creates a client using a pre-existing token (such as `VAULT_TOKEN`).
    ///
    /// Because this client did not acquire the token via login, [`VaultClient::revoke`] and
    /// [`Drop`] will never revoke it.
    ///
    /// # Errors
    ///
    /// [`SecretError::File`] when the CA certificate file cannot be read; [`SecretError::Vault`] when it is not valid PEM.
    pub fn with_token(
        url: &str,
        token: SecretString,
        ca_certificate: Option<&Path>,
    ) -> Result<Self, SecretError> {
        let url = normalize_url(url);
        let agent = build_agent(&url, ca_certificate)?;
        Ok(Self {
            url,
            agent,
            token,
            revoke_on_drop: false,
            revoke_timeout: REVOKE_TIMEOUT,
        })
    }

    #[cfg(test)]
    const fn with_revoke_timeout(mut self, timeout: Duration) -> Self {
        self.revoke_timeout = timeout;
        self
    }

    /// Reads all string values of a KV v2 secret.
    ///
    /// # Errors
    ///
    /// [`SecretError::Vault`] when the request fails (HTTP error, unreachable server) or the response has no `data.data`
    /// object.
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

    fn revoke_if_owned(&mut self) {
        if !self.revoke_on_drop {
            return;
        }
        self.revoke_on_drop = false;
        let url = format!("{}/v1/auth/token/revoke-self", self.url);
        if let Err(error) = self
            .agent
            .post(&url)
            .header("X-Vault-Token", self.token.expose_secret())
            .config()
            .timeout_global(Some(self.revoke_timeout))
            .build()
            .send_empty()
        {
            tracing::warn!(%error, "could not revoke the Vault token");
        }
    }

    /// Revokes a login-acquired token at most once; failures are logged only.
    pub fn revoke(mut self) {
        self.revoke_if_owned();
    }
}

impl Drop for VaultClient {
    fn drop(&mut self) {
        self.revoke_if_owned();
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
    /// Reads every path of the KV v2 `mount` through `client`; for a key present in several paths, the first path wins.
    ///
    /// # Errors
    ///
    /// [`SecretError::Vault`] when any path cannot be read (see [`VaultClient::read_kv2`]).
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use tiny_http::{Response, Server, StatusCode};
    use tracing::Subscriber;
    use tracing::field::{Field, Visit};
    use tracing_subscriber::layer::{Context as LayerContext, Layer, SubscriberExt as _};

    use super::*;

    type RecordedRequests = Arc<Mutex<Vec<(String, String, Option<String>)>>>;

    struct FakeVault {
        base_url: String,
        requests: RecordedRequests,
        stop: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FakeVault {
        fn start(revoke_status: u16) -> Self {
            Self::spawn(revoke_status, false)
        }

        fn start_hanging_revoke() -> Self {
            Self::spawn(204, true)
        }

        fn spawn(revoke_status: u16, hang_revoke: bool) -> Self {
            let server = Server::http("127.0.0.1:0").unwrap();
            let base_url = format!("http://{}", server.server_addr());
            let requests: RecordedRequests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));

            let recorded = Arc::clone(&requests);
            let stop_flag = Arc::clone(&stop);
            let handle = thread::spawn(move || {
                while !stop_flag.load(Ordering::Acquire) {
                    let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(15)) else {
                        continue;
                    };
                    let method = request.method().as_str().to_owned();
                    let url = request.url().to_owned();
                    let token = request
                        .headers()
                        .iter()
                        .find(|h| {
                            h.field
                                .as_str()
                                .as_str()
                                .eq_ignore_ascii_case("x-vault-token")
                        })
                        .map(|h| h.value.as_str().to_owned());
                    recorded
                        .lock()
                        .unwrap()
                        .push((method.clone(), url.clone(), token));

                    let (status, body) = match (method.as_str(), url.as_str()) {
                        ("POST", "/v1/auth/approle/login") => (
                            200,
                            r#"{"auth":{"client_token":"tok-secret-123"}}"#.to_owned(),
                        ),
                        ("GET", "/v1/kv-other/data/app") => {
                            (200, r#"{"data":{"data":{"DB_PASS":"pw"}}}"#.to_owned())
                        }
                        ("POST", "/v1/auth/token/revoke-self") => {
                            if hang_revoke {
                                while !stop_flag.load(Ordering::Acquire) {
                                    thread::sleep(Duration::from_millis(15));
                                }
                            }
                            (revoke_status, String::new())
                        }
                        _ => (404, String::new()),
                    };
                    let response = Response::from_string(body).with_status_code(StatusCode(status));
                    let _ = request.respond(response);
                }
            });

            Self {
                base_url,
                requests,
                stop,
                handle: Some(handle),
            }
        }

        fn revoke_calls(&self) -> Vec<Option<String>> {
            self.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(m, u, _)| m == "POST" && u == "/v1/auth/token/revoke-self")
                .map(|(_, _, tok)| tok.clone())
                .collect()
        }
    }

    impl Drop for FakeVault {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    fn normalize_url_table() {
        let cases = [
            ("vault.example.com:8200", "https://vault.example.com:8200"),
            ("vault.example.com:8200/", "https://vault.example.com:8200"),
            (
                "https://vault.example.com:8200///",
                "https://vault.example.com:8200",
            ),
            ("http://127.0.0.1:8200/", "http://127.0.0.1:8200"),
            ("  http://localhost:8200/  ", "http://localhost:8200"),
            ("HTTPS://h", "https://h"),
            ("Http://h:8200/", "http://h:8200"),
            ("  h:8200/  ", "https://h:8200"),
            ("", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_url(input), expected, "input: {input:?}");
            assert_eq!(
                VaultClient::normalize_url(input),
                expected,
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn login_and_drop_revokes_token_once() {
        let vault = FakeVault::start(204);
        let role = SecretString::from("role".to_owned());
        let secret = SecretString::from("secret".to_owned());
        {
            let base_url = format!("{}/", vault.base_url);
            let client = VaultClient::login_approle(&AppRoleCredentials {
                url: &base_url,
                role_id: &role,
                secret_id: &secret,
                ca_certificate: None,
            })
            .unwrap();
            let kv = client.read_kv2("kv-other", "app").unwrap();
            assert_eq!(kv["DB_PASS"].expose_secret(), "pw");
        }
        assert_eq!(vault.revoke_calls(), vec![Some("tok-secret-123".into())]);
    }

    #[test]
    fn explicit_revoke_then_drop_revokes_token_only_once() {
        let vault = FakeVault::start(204);
        let role = SecretString::from("role".to_owned());
        let secret = SecretString::from("secret".to_owned());
        let client = VaultClient::login_approle(&AppRoleCredentials {
            url: &vault.base_url,
            role_id: &role,
            secret_id: &secret,
            ca_certificate: None,
        })
        .unwrap();
        client.revoke();
        assert_eq!(vault.revoke_calls(), vec![Some("tok-secret-123".into())]);
    }

    #[test]
    fn external_token_is_never_revoked_on_drop_or_explicit_revoke() {
        let vault = FakeVault::start(204);
        let tok = SecretString::from("external-token-xyz".to_owned());
        {
            let client = VaultClient::with_token(&vault.base_url, tok.clone(), None).unwrap();
            let kv = client.read_kv2("kv-other", "app").unwrap();
            assert_eq!(kv["DB_PASS"].expose_secret(), "pw");
        }
        let client = VaultClient::with_token(&vault.base_url, tok, None).unwrap();
        client.revoke();
        assert!(
            vault.revoke_calls().is_empty(),
            "external token must never be revoked: {:?}",
            vault.revoke_calls()
        );
    }

    #[derive(Default)]
    struct WarningCapture(Vec<String>);

    impl Visit for WarningCapture {
        fn record_str(&mut self, field: &Field, value: &str) {
            self.0.push(format!("{}={value}", field.name()));
        }

        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.0.push(format!("{}={value:?}", field.name()));
        }
    }

    struct WarnLayer(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber> Layer<S> for WarnLayer {
        fn on_event(&self, event: &tracing::Event<'_>, _ctx: LayerContext<'_, S>) {
            if *event.metadata().level() != tracing::Level::WARN {
                return;
            }
            let mut capture = WarningCapture::default();
            event.record(&mut capture);
            self.0.lock().unwrap().push(capture.0.join(" "));
        }
    }

    #[test]
    fn revoke_500_logs_one_warning_without_token_and_does_not_panic() {
        let vault = FakeVault::start(500);
        let role = SecretString::from("role".to_owned());
        let secret = SecretString::from("secret".to_owned());
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(WarnLayer(Arc::clone(&warnings)));

        tracing::subscriber::with_default(subscriber, || {
            let client = VaultClient::login_approle(&AppRoleCredentials {
                url: &vault.base_url,
                role_id: &role,
                secret_id: &secret,
                ca_certificate: None,
            })
            .unwrap();
            drop(client);
        });

        assert_eq!(vault.revoke_calls(), vec![Some("tok-secret-123".into())]);
        let captured = warnings.lock().unwrap();
        assert_eq!(captured.len(), 1, "expected 1 warning, got: {captured:?}");
        assert!(
            captured[0].contains("could not revoke the Vault token"),
            "unexpected warning: {}",
            captured[0]
        );
        assert!(
            !captured[0].contains("tok-secret-123"),
            "token leaked in warning: {}",
            captured[0]
        );
    }

    #[test]
    fn unanswered_revoke_times_out_within_bound_and_logs_one_warning() {
        use std::sync::mpsc;
        use std::time::Instant;

        assert!(REVOKE_TIMEOUT <= Duration::from_secs(5));
        let vault = FakeVault::start_hanging_revoke();
        let role = SecretString::from("role".to_owned());
        let secret = SecretString::from("secret".to_owned());
        let client = VaultClient::login_approle(&AppRoleCredentials {
            url: &vault.base_url,
            role_id: &role,
            secret_id: &secret,
            ca_certificate: None,
        })
        .unwrap()
        .with_revoke_timeout(Duration::from_millis(200));

        let warnings = Arc::new(Mutex::new(Vec::new()));
        let warnings_bg = Arc::clone(&warnings);
        let (tx, rx) = mpsc::channel();
        let drop_handle = thread::spawn(move || {
            let subscriber = tracing_subscriber::registry().with(WarnLayer(warnings_bg));
            let started = Instant::now();
            tracing::subscriber::with_default(subscriber, move || drop(client));
            let _ = tx.send(started.elapsed());
        });

        let elapsed = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("revoke on Drop hung past timeout bound");
        drop_handle.join().unwrap();
        assert!(
            elapsed < Duration::from_secs(2),
            "revoke drop took too long: {elapsed:?}"
        );
        assert_eq!(vault.revoke_calls(), vec![Some("tok-secret-123".into())]);
        let captured = warnings.lock().unwrap();
        assert_eq!(captured.len(), 1, "expected 1 warning, got: {captured:?}");
        assert!(
            captured[0].contains("could not revoke the Vault token"),
            "unexpected warning: {}",
            captured[0]
        );
        assert!(
            !captured[0].contains("tok-secret-123"),
            "token leaked in warning: {}",
            captured[0]
        );
    }
}
