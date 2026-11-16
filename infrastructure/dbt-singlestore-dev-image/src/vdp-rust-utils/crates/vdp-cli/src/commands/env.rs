use std::sync::Arc;

use anyhow::Result;
use vdp_core::{Context, Outcome};

use crate::args::EnvCommand;
use crate::bootstrap::{ResolvedSecrets, VaultStatus};

pub(super) fn run(ctx: &Context, command: &EnvCommand) -> Result<Outcome> {
    match command {
        EnvCommand::Show => show(ctx),
        EnvCommand::Doctor => doctor(ctx),
    }
}

fn show(ctx: &Context) -> Result<Outcome> {
    let settings = ctx.config().environment(ctx.environment());
    println!("environment     {}", ctx.environment());
    match ctx.project_opt() {
        Some(project) => println!("project         {}", project.root.display()),
        None => println!("project         (none found from {})", ctx.cwd().display()),
    }
    println!("configuration   {}", ctx.config().origins().join(" + "));
    match ctx.ca_certificate() {
        Some(ca) => {
            let state = if ca.is_file() { "" } else { "  (missing)" };
            println!("ca certificate  {}{state}", ca.display());
        }
        None => println!("ca certificate  none configured"),
    }
    println!("vault           {}", settings.vault_url);

    let secrets = ctx.service::<Arc<ResolvedSecrets>>()?;
    match &secrets.vault {
        VaultStatus::Read(paths) => println!("vault status    read {}", paths.join(", ")),
        VaultStatus::Skipped(reason) => println!("vault status    skipped: {reason}"),
        VaultStatus::Unavailable(reason) => println!("vault status    unavailable: {reason}"),
    }
    println!(
        "secret sources  {}",
        secrets.chain.provider_names().join(" > ")
    );
    for key in vdp_secrets::SecretProvider::keys(secrets.as_ref()) {
        let source = secrets.chain.source_of(&key).unwrap_or_default();
        println!("  {key:<32} from {source}");
    }
    Ok(Outcome::Success)
}

struct Check {
    ok: bool,
    name: &'static str,
    detail: String,
}

fn check(ok: bool, name: &'static str, detail: impl Into<String>) -> Check {
    Check {
        ok,
        name,
        detail: detail.into(),
    }
}

/// The environment's CA certificate exists; an empty `ca_certificate` (no TLS, no Vault) needs none.
fn ca_check(ctx: &Context) -> Check {
    match ctx.ca_certificate() {
        Some(ca) => check(ca.is_file(), "ca certificate", ca.display().to_string()),
        None => check(
            true,
            "ca certificate",
            format!(
                "none configured ([environments.{}].ca_certificate is empty)",
                ctx.environment()
            ),
        ),
    }
}

/// The ADBC driver dbt loads for SingleStore: `lib/dbt/libadbc_driver_singlestore.so` in a directory above the
/// engine binary (dbt walks up to five levels from its own executable).
fn find_driver(binary: &std::path::Path) -> Option<std::path::PathBuf> {
    let binary = std::fs::canonicalize(binary).ok()?;
    binary
        .ancestors()
        .skip(1)
        .take(6)
        .map(|dir| dir.join("lib/dbt/libadbc_driver_singlestore.so"))
        .find(|candidate| candidate.is_file())
}

fn doctor(ctx: &Context) -> Result<Outcome> {
    let mut checks = Vec::new();
    match super::dbt::engine_binary(ctx.config()) {
        Ok(binary) => {
            let version = std::process::Command::new(&binary)
                .arg("--version")
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
            checks.push(check(
                version.is_some(),
                "dbt engine",
                format!(
                    "{} ({})",
                    binary.display(),
                    version.as_deref().unwrap_or("does not run")
                ),
            ));
            let embedded = vdp_dbt::Grammar::embedded();
            if let Some(version) = &version {
                checks.push(check(
                    true,
                    "argument grammar",
                    if *version == embedded.dbt_version {
                        format!("matches the embedded copy ({version})")
                    } else {
                        format!(
                            "derived from the engine; embedded copy is for {}",
                            embedded.dbt_version
                        )
                    },
                ));
            }
            match find_driver(&binary) {
                Some(driver) => checks.push(check(
                    true,
                    "singlestore driver",
                    driver.display().to_string(),
                )),
                None => checks.push(check(
                    false,
                    "singlestore driver",
                    "lib/dbt/libadbc_driver_singlestore.so not found above the engine binary",
                )),
            }
        }
        Err(error) => checks.push(check(false, "dbt engine", error.to_string())),
    }
    let settings: vdp_dbt::DbtSettings = ctx.config().section(vdp_dbt::DbtSettings::SECTION)?;
    let experimental = std::env::var_os("DBT_ALLOW_EXPERIMENTAL_ADAPTERS").is_some()
        || settings.env.contains_key("DBT_ALLOW_EXPERIMENTAL_ADAPTERS");
    checks.push(check(
        experimental,
        "experimental adapters",
        "DBT_ALLOW_EXPERIMENTAL_ADAPTERS (the SingleStore adapter is experimental in dbt v2)",
    ));
    checks.push(ca_check(ctx));
    match ctx.project_opt() {
        Some(project) => {
            checks.push(check(
                true,
                "dbt project",
                project.root.display().to_string(),
            ));
        }
        None => checks.push(check(
            false,
            "dbt project",
            "no dbt_project.yml here or above",
        )),
    }
    match ctx.service::<Arc<ResolvedSecrets>>() {
        Ok(secrets) => {
            let (ok, detail) = match &secrets.vault {
                VaultStatus::Read(paths) => (true, format!("read {}", paths.join(", "))),
                VaultStatus::Skipped(reason) => (true, format!("skipped: {reason}")),
                VaultStatus::Unavailable(reason) => (false, reason.clone()),
            };
            checks.push(check(ok, "vault", detail));
        }
        Err(error) => checks.push(check(false, "vault", error.to_string())),
    }
    for c in &checks {
        println!(
            "{} {:<22} {}",
            if c.ok { "ok  " } else { "FAIL" },
            c.name,
            c.detail
        );
    }
    Ok(Outcome::from_success(checks.iter().all(|c| c.ok)))
}
