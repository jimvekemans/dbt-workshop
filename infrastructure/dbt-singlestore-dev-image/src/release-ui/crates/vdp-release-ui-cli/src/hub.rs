//! `release serve`: starts the release hub.
//!
//! [`serve`] is the whole command. The hub knows dbt releases only; a binary that serves further repositories
//! passes a factory ([`ExtensionFactory`]) that builds their [`HubExtension`]s (`vdp-vaultspeed` adds the VaultSpeed
//! releases). `vdp-release-ui` itself passes [`no_extensions`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{Context, Outcome};
use vdp_dbt::DbtEngine;
use vdp_jira::{IssueTracker, JiraSettings};
use vdp_notify::{NotifySettings, SmtpMailer};
use vdp_release::ReleaseSettings;
use vdp_release_hub::{
    GitLab, Hub, HubConfig, HubExtension, HubServer, HubSettings, ServeOptions, Validator,
    non_empty,
};
use vdp_secrets::SecretProvider;

use crate::args::ServeArgs;

/// What an extension factory may use of the hub being started.
#[derive(Debug)]
pub struct ExtensionInputs<'a> {
    /// `[release_hub]`, whose `extensions` table holds the segments' settings (see [`HubSettings::extension`]).
    pub settings: &'a HubSettings,
    /// Whether sign-in is on, so the extension must know its GitLab.
    pub sign_in: bool,
    /// The Jira client, when Jira is configured.
    pub tracker: Option<Arc<dyn IssueTracker>>,
}

/// Builds the hub extensions of a binary (further repositories next to the dbt one).
pub type ExtensionFactory<'a> =
    &'a dyn Fn(&Context, &ExtensionInputs<'_>) -> Result<Vec<Arc<dyn HubExtension>>>;

/// The factory of `vdp-release-ui`: no extensions. A `[release_hub.<segment>]` table that nothing serves is said, not ignored.
///
/// # Errors
///
/// Never fails: the `Result` is the signature of an [`ExtensionFactory`].
#[allow(clippy::unnecessary_wraps, reason = "an `ExtensionFactory`")]
pub fn no_extensions(
    _: &Context,
    inputs: &ExtensionInputs<'_>,
) -> Result<Vec<Arc<dyn HubExtension>>> {
    for segment in inputs.settings.extensions.keys() {
        tracing::warn!(
            "[release_hub.{segment}] is set, but this binary serves no such release extension"
        );
    }
    Ok(Vec::new())
}

/// dbt with `[release] validate` in the clone; the first error lines are the reason.
fn validator(ctx: &Context, args: Vec<String>) -> Result<Validator> {
    let engine = ctx
        .service::<Arc<dyn DbtEngine>>()
        .context("--validate needs dbt")?;
    Ok(Arc::new(move |dir: &Path| {
        vdp_cli::commands::run_dbt_validation(
            engine.as_ref(),
            dir,
            args.iter().map(Into::into).collect(),
        )
    }))
}

/// `release serve`, with the extensions `extensions` builds (`vdp-release-ui` passes [`no_extensions`]).
///
/// # Errors
///
/// An error when a configuration section is invalid, the UI directory has no `index.html`, secrets or the validator
/// cannot be set up, the GitLab sign-in settings are invalid, `extensions` fails, or the server cannot listen on the
/// address. A missing Jira connection is not an error: analyses then lack Jira data. The call returns when the server
/// stops.
pub fn serve(ctx: &Context, args: &ServeArgs, extensions: ExtensionFactory<'_>) -> Result<Outcome> {
    let ServeArgs {
        repo_dir,
        bind,
        port,
        ui_dir,
        cors_origin,
        validate,
    } = args;
    let release: ReleaseSettings = ctx.config().section(ReleaseSettings::SECTION)?;
    let settings: HubSettings = ctx.config().section(HubSettings::SECTION)?;
    let jira: JiraSettings = ctx.config().section(JiraSettings::SECTION)?;
    let notify: NotifySettings = ctx.config().section(NotifySettings::SECTION)?;

    let repo_dir = repo_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(&settings.repo_dir));
    let repo_dir = ctx.cwd().join(repo_dir);
    let ui_dir = ui_dir
        .clone()
        .or_else(|| non_empty(&settings.ui_dir).map(PathBuf::from))
        .map(|d| ctx.cwd().join(d));
    if let Some(dir) = &ui_dir
        && !dir.join("index.html").is_file()
    {
        bail!(
            "{} has no index.html (build the frontend first)",
            dir.display()
        );
    }
    let addr = format!(
        "{}:{}",
        bind.clone().unwrap_or(settings.bind.clone()),
        port.unwrap_or(settings.port)
    );

    let tracker = ctx
        .service::<Arc<dyn IssueTracker>>()
        .map_err(|e| vdp_core::display_chain(&e));
    if let Err(why) = &tracker {
        tracing::warn!("Jira is not available, analyses will lack Jira data: {why}");
    }
    let secrets = ctx.service::<Arc<dyn SecretProvider>>()?;
    let validator = (*validate || settings.validate)
        .then(|| validator(ctx, release.validate.clone()))
        .transpose()?;
    let gitlab = GitLab::from_hub_settings(&settings)?;
    let extensions = extensions(
        ctx,
        &ExtensionInputs {
            settings: &settings,
            sign_in: gitlab.is_some(),
            tracker: tracker.as_ref().ok().cloned(),
        },
    )?;
    let hub = Hub::new(
        HubConfig {
            repo_dir: repo_dir.clone(),
            repo_url: settings.repo_url.clone(),
            release,
            jira_base_url: jira.base_url,
            excluded_fix_versions: jira.excluded_fix_versions,
            default_recipients: notify.default_recipients.clone(),
            keep_jobs: settings.keep_jobs,
            git_timeout: (settings.git_timeout_secs > 0)
                .then(|| std::time::Duration::from_secs(settings.git_timeout_secs)),
            conflict_timeout: std::time::Duration::from_secs(settings.conflict_timeout_secs),
            state_dir: non_empty(&settings.state_dir).map(PathBuf::from),
            gitlab,
        },
        vdp_release_hub::HubServices {
            tracker,
            mailer: Arc::new(SmtpMailer::new(notify, secrets.as_ref())),
            validator,
            extensions,
        },
    );
    let server = HubServer::bind(
        &addr,
        ServeOptions {
            cors_origin: cors_origin
                .clone()
                .or_else(|| non_empty(&settings.cors_origin)),
            ui_dir: ui_dir.clone(),
        },
    )?;
    let shown = server.local_addr().map_or(addr.clone(), |a| a.to_string());
    eprintln!(
        "release hub on http://{shown}{} (repository {}{})",
        vdp_release_hub::http::API,
        repo_dir.display(),
        ui_dir
            .as_ref()
            .map(|d| format!(", UI from {}", d.display()))
            .unwrap_or_default()
    );
    server.run(&hub);
    Ok(Outcome::Success)
}
