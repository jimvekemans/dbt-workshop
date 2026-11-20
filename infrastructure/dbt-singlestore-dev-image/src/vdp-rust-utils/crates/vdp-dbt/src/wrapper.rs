//! The `dbt` command: dbt v2 plus the organisation's environment, the selection guard and helpful hints.
//!
//! Replaces the Python dbt wrapper of the old utilities, with one overriding rule:
//! every argv dbt accepts reaches dbt with the same meaning. Tokens are forwarded byte for byte; the only values
//! that ever change are `--args`/`--vars` values dbt would misread or reject (see [`crate::argv::yaml_arg`]).

use std::ffi::OsString;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use vdp_core::{Context, Outcome};
use vdp_secrets::{ExposeSecret, SecretProvider, SecretString};

use crate::argv::{self, Scan};
use crate::flags::{Grammar, cache};
use crate::guard::{self, Verdict};
use crate::{DbtEngine, DbtError, DbtSettings, Invocation, SubprocessDbt, missing_sources};

/// The grammar to read argv with, per `[dbt].grammar`.
pub fn grammar_for(binary: &Path, settings: &DbtSettings) -> Grammar {
    if settings.grammar == "embedded" {
        Grammar::embedded()
    } else {
        cache::for_binary(binary)
    }
}

/// Whether the invocation needs no secrets (help, version, completions, ...).
pub fn is_offline(scan: &Scan, settings: &DbtSettings) -> bool {
    scan.uses("help")
        || scan.uses("version")
        || scan
            .command()
            .is_none_or(|command| settings.offline_commands.iter().any(|c| c == command))
}

/// The variables dbt gets on top of the inherited environment: `[dbt].env` defaults (unless already set),
/// `VDP_ENV`, `VDP_SSL_CA` (the environment's CA, for `ssl_ca:` in profiles.yml) and every secret the dotenv
/// file and Vault provide.
pub fn child_env(
    ctx: &Context,
    settings: &DbtSettings,
    secrets: Option<&dyn SecretProvider>,
) -> Vec<(String, SecretString)> {
    let mut env: Vec<(String, SecretString)> = settings
        .env
        .iter()
        .filter(|(key, _)| std::env::var_os(key).is_none())
        .map(|(k, v)| (k.clone(), SecretString::from(v.clone())))
        .collect();
    env.push((
        "VDP_ENV".into(),
        SecretString::from(ctx.environment().to_string()),
    ));
    if let Some(ca) = ctx.ca_certificate().filter(|ca| ca.is_file()) {
        env.push((
            "VDP_SSL_CA".into(),
            SecretString::from(ca.display().to_string()),
        ));
    }
    if let Some(secrets) = secrets {
        env.extend(vdp_secrets::child_env(secrets));
    }
    env
}

/// Everything the wrapper decided before running dbt.
#[derive(Debug)]
pub struct Prepared {
    pub args: Vec<OsString>,
    pub verdict: Verdict,
    pub notices: Vec<argv::Notice>,
}

/// Inputs for [`prepare`].
#[derive(Clone, Copy)]
pub struct PrepareContext<'a> {
    pub args: &'a [OsString],
    pub scan: &'a Scan,
    pub settings: &'a DbtSettings,
    pub secrets: Option<&'a dyn SecretProvider>,
}

impl std::fmt::Debug for PrepareContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrepareContext")
            .field("args", &self.args)
            .field("scan", &self.scan)
            .field("settings", &self.settings)
            .field("secrets", &self.secrets.map(|_| "<secrets>"))
            .finish()
    }
}

/// Target binary and arguments for [`run`].
#[derive(Debug, Clone, Copy)]
pub struct RunRequest<'a> {
    pub binary: &'a Path,
    pub args: &'a [OsString],
    pub scan: &'a Scan,
}

/// Applies the guard and the value fixes.
pub fn prepare(ctx: PrepareContext<'_>) -> Prepared {
    let user = ctx
        .secrets
        .and_then(|s| s.get(&ctx.settings.user_secret))
        .map(|u| u.expose_secret().to_owned());
    let verdict = guard::decide(ctx.scan, ctx.settings, user.as_deref());
    let (args, notices) = argv::prepare(ctx.args, ctx.scan);
    Prepared {
        args,
        verdict,
        notices,
    }
}

/// Runs the wrapper for `request.args` (without the program name), already scanned with the engine's grammar.
///
/// # Errors
///
/// [`DbtError::Grammar`] when the `[dbt]` config section is invalid; [`DbtError::Spawn`] when dbt cannot be started;
/// [`DbtError::Signals`] when the interactive run cannot install its signal handlers.
/// A refused command and dbt's own failures are not errors: they come back as the [`Outcome`] (an exit code).
pub fn run(ctx: &Context, request: RunRequest<'_>) -> Result<Outcome, DbtError> {
    let RunRequest { binary, args, scan } = request;
    let settings: DbtSettings = ctx
        .config()
        .section(DbtSettings::SECTION)
        .map_err(|e| DbtError::Grammar(vdp_core::display_chain(&e)))?;
    let offline = is_offline(scan, &settings);
    let secrets = if offline {
        None
    } else {
        match ctx.service::<std::sync::Arc<dyn SecretProvider>>() {
            Ok(secrets) => Some(secrets),
            Err(error) => {
                tracing::warn!(error = %vdp_core::display_chain(&error), "running dbt without the VDP secrets");
                None
            }
        }
    };
    let secrets_ref = secrets.as_deref();

    let prepared = prepare(PrepareContext {
        args,
        scan,
        settings: &settings,
        secrets: secrets_ref,
    });
    if let Verdict::Refused { command } = &prepared.verdict {
        eprintln!(
            "vdp: 'dbt {command}' without a selector would build the whole project. Select nodes with \
             --select/-s or --selector, or run it as a service user (…{}, not {}…).",
            settings.service_user_suffix, settings.personal_user_prefix
        );
        return Ok(Outcome::Exit(1));
    }
    for notice in &prepared.notices {
        tracing::warn!(
            "{} {:?} → {:?}: {}",
            notice.flag,
            notice.before,
            notice.after,
            notice.reason
        );
    }

    let log = log_file(ctx, args, scan);
    let log_start = log
        .as_deref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len());

    let engine = SubprocessDbt::new(binary.to_owned(), child_env(ctx, &settings, secrets_ref));
    let done = engine.invoke(&Invocation {
        args: prepared.args,
        cwd: None,
        capture: false,
    })?;

    if !done.success()
        && settings.missing_source_hint
        && let Some(log) = &log
    {
        hint_missing_sources(log, log_start);
    }
    Ok(Outcome::Exit(done.exit_code()))
}

/// `dbt.log` of this invocation: `--log-path`, else `$DBT_LOG_PATH`, else `<project>/logs`.
fn log_file(ctx: &Context, args: &[OsString], scan: &Scan) -> Option<PathBuf> {
    let dir = scan
        .value(args, "log-path")
        .map(|p| ctx.cwd().join(p))
        .or_else(|| std::env::var_os("DBT_LOG_PATH").map(|p| ctx.cwd().join(p)))
        .or_else(|| ctx.project_opt().map(|p| p.root.join("logs")))?;
    Some(dir.join("dbt.log"))
}

fn hint_missing_sources(log: &Path, start: u64) {
    let Ok(mut file) = std::fs::File::open(log) else {
        return;
    };
    let mut text = String::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_string(&mut text).is_err() {
        return;
    }
    let missing = missing_sources(&text);
    if missing.is_empty() {
        return;
    }
    let names: Vec<String> = missing
        .iter()
        .map(super::MissingSource::qualified)
        .collect();
    eprintln!(
        "\nhint: dbt reports {} undefined source table(s). Add them from the database with:\n  vdp sources add {}\n  (or: vdp sources add --missing)",
        names.len(),
        names.join(" ")
    );
}

/// An engine for modules: the subprocess with the same environment the wrapper gives dbt.
///
/// # Errors
///
/// [`DbtError::Grammar`] when the `[dbt]` config section is invalid.
pub fn module_engine(
    ctx: &Context,
    binary: PathBuf,
    secrets: Option<&dyn SecretProvider>,
) -> Result<SubprocessDbt, DbtError> {
    let settings: DbtSettings = ctx
        .config()
        .section(DbtSettings::SECTION)
        .map_err(|e| DbtError::Grammar(vdp_core::display_chain(&e)))?;
    Ok(SubprocessDbt::new(
        binary,
        child_env(ctx, &settings, secrets),
    ))
}

#[cfg(test)]
mod tests {
    use vdp_core::{Config, Environment};

    use super::*;

    fn value<'a>(env: &'a [(String, SecretString)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.expose_secret())
    }

    /// `dev` is an alias of `LDV`: dbt gets `VDP_ENV=LDV`, and vdp adds nothing for `DBT_DEPLOY_TARGET`, so the
    /// child sees the value its parent has, `dev` for the workshop.
    #[test]
    fn vdp_env_is_the_canonical_name_and_the_deploy_target_is_left_alone() {
        let ctx = Context::builder(Config::default())
            .environment(Environment::named("dev").unwrap())
            .build();
        let env = child_env(&ctx, &DbtSettings::default(), None);
        assert_eq!(value(&env, "VDP_ENV"), Some("LDV"));
        assert_eq!(value(&env, "DBT_DEPLOY_TARGET"), None);
    }
}
