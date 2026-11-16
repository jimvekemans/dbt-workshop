//! `vdp release serve`: the release hub.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use vdp_core::{Context, Outcome};
use vdp_dbt::{DbtEngine, Invocation};
use vdp_jira::{IssueTracker, JiraSettings};
use vdp_notify::{NotifySettings, SmtpMailer};
use vdp_release::ReleaseSettings;
use vdp_release_hub::{GitLab, Hub, HubConfig, HubServer, HubSettings, ServeOptions, Validator};
use vdp_secrets::SecretProvider;

use crate::args::ReleaseCommand;

/// The VaultSpeed side (`[release_hub.vaultspeed]`, `[vaultspeed_release]`, `[vaultspeed]`), when enabled.
fn vaultspeed_config(
    ctx: &Context,
    settings: &HubSettings,
    sign_in: bool,
) -> Result<Option<vdp_release_hub::VsConfig>> {
    let vs = &settings.vaultspeed;
    if !vs.enabled {
        return Ok(None);
    }
    let gitlab = if sign_in {
        let mut gitlab = GitLab::from_repo_url(&vs.repo_url).unwrap_or(GitLab {
            url: String::new(),
            project: String::new(),
            push_url: String::new(),
        });
        if let Some(url) = non_empty(&vs.gitlab_url).or_else(|| non_empty(&settings.gitlab_url)) {
            gitlab.url = url;
        }
        if let Some(project) = non_empty(&vs.gitlab_project) {
            gitlab.project = project;
        }
        if let Some(push) = non_empty(&vs.push_url) {
            gitlab.push_url = push;
        }
        if gitlab.url.is_empty() || gitlab.project.is_empty() || gitlab.push_url.is_empty() {
            bail!(
                "[release_hub.vaultspeed] needs gitlab_url, gitlab_project and push_url (not derivable from {})",
                vs.repo_url
            );
        }
        Some(gitlab)
    } else {
        None
    };
    Ok(Some(vdp_release_hub::VsConfig {
        repo_dir: ctx.cwd().join(&vs.repo_dir),
        repo_url: vs.repo_url.clone(),
        gitlab,
        release: ctx
            .config()
            .section(vdp_vaultspeed_release::VsReleaseSettings::SECTION)?,
        vaultspeed: ctx
            .config()
            .section(vdp_vaultspeed::VaultspeedSettings::SECTION)?,
        keep_jobs: settings.keep_jobs,
        git_timeout: (settings.git_timeout_secs > 0)
            .then(|| std::time::Duration::from_secs(settings.git_timeout_secs)),
        conflict_timeout: std::time::Duration::from_secs(settings.conflict_timeout_secs),
        state_dir: non_empty(&settings.state_dir).map(|d| PathBuf::from(d).join("vaultspeed")),
    }))
}

fn non_empty(value: &str) -> Option<String> {
    Some(value.trim().to_owned()).filter(|v| !v.is_empty())
}

/// dbt with `[release] validate` in the clone; the first error lines are the reason.
fn validator(ctx: &Context, args: Vec<String>) -> Result<Validator> {
    let engine = ctx
        .service::<Arc<dyn DbtEngine>>()
        .context("--validate needs dbt")?;
    Ok(Arc::new(move |dir: &Path| {
        let done = engine
            .invoke(&Invocation {
                args: args.iter().map(Into::into).collect(),
                cwd: Some(dir.to_owned()),
                capture: true,
            })
            .map_err(|e| e.to_string())?;
        if done.success() {
            return Ok(());
        }
        let output = done.stdout_text();
        let errors: Vec<&str> = output
            .lines()
            .filter(|l| l.contains("[error]"))
            .take(10)
            .collect();
        Err(if errors.is_empty() {
            format!("dbt exited with {}", done.exit_code())
        } else {
            errors.join("\n")
        })
    }))
}

/// GitLab sign-in from `[release_hub]` (`None` with `auth = "none"`).
fn gitlab(settings: &HubSettings) -> Result<Option<GitLab>> {
    Ok(match settings.auth.as_str() {
        "none" => {
            tracing::warn!(
                "sign-in is off ([release_hub].auth = \"none\"): anyone who reaches the hub can release"
            );
            None
        }
        "gitlab" => {
            let mut gitlab = GitLab::from_repo_url(&settings.repo_url).unwrap_or(GitLab {
                url: String::new(),
                project: String::new(),
                push_url: String::new(),
            });
            if let Some(url) = non_empty(&settings.gitlab_url) {
                gitlab.url = url;
            }
            if let Some(project) = non_empty(&settings.gitlab_project) {
                gitlab.project = project;
            }
            if let Some(push) = non_empty(&settings.push_url) {
                gitlab.push_url = push;
            }
            if gitlab.url.is_empty() || gitlab.project.is_empty() || gitlab.push_url.is_empty() {
                bail!(
                    "[release_hub].auth = \"gitlab\" needs gitlab_url, gitlab_project and push_url (not derivable from {})",
                    settings.repo_url
                );
            }
            Some(gitlab)
        }
        other => bail!("[release_hub].auth must be \"gitlab\" or \"none\", not {other:?}"),
    })
}

pub(super) fn serve(
    ctx: &Context,
    release: ReleaseSettings,
    command: &ReleaseCommand,
) -> Result<Outcome> {
    let ReleaseCommand::Serve {
        repo_dir,
        bind,
        port,
        ui_dir,
        cors_origin,
        validate,
    } = command
    else {
        unreachable!("dispatched on Serve")
    };
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
        .map_err(|e| e.to_string());
    if let Err(why) = &tracker {
        tracing::warn!("Jira is not available, analyses will lack Jira data: {why}");
    }
    let secrets = ctx.service::<Arc<dyn SecretProvider>>()?;
    let mailer = SmtpMailer::new(notify.clone(), secrets.as_ref());
    let validator = if *validate || settings.validate {
        Some(validator(ctx, release.validate.clone())?)
    } else {
        None
    };
    let gitlab = gitlab(&settings)?;
    let hub = Hub::new(
        HubConfig {
            repo_dir: repo_dir.clone(),
            repo_url: settings.repo_url.clone(),
            release,
            jira_base_url: jira.base_url,
            excluded_fix_versions: jira.excluded_fix_versions,
            default_recipients: notify.default_recipients,
            keep_jobs: settings.keep_jobs,
            git_timeout: (settings.git_timeout_secs > 0)
                .then(|| std::time::Duration::from_secs(settings.git_timeout_secs)),
            conflict_timeout: std::time::Duration::from_secs(settings.conflict_timeout_secs),
            state_dir: non_empty(&settings.state_dir).map(PathBuf::from),
            vaultspeed: vaultspeed_config(ctx, &settings, gitlab.is_some())?,
            gitlab,
        },
        tracker,
        Arc::new(mailer),
        validator,
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
