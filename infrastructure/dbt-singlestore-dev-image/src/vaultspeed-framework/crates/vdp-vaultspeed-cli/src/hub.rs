//! `release serve` with the VaultSpeed releases: the extension of the release hub that steers the framework
//! repository (`[release_hub.vaultspeed]`, `[vaultspeed_release]`, `[vaultspeed]`), when enabled.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;

use vdp_core::Context;
use vdp_release_hub::{GitLab, GitLabSource, HubExtension, non_empty};
use vdp_release_ui_cli::hub::ExtensionInputs;
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_hub::{SEGMENT, VaultspeedExtension, VaultspeedHubSettings, VsConfig};
use vdp_vaultspeed_release::VsReleaseSettings;

fn build_config(
    ctx: &Context,
    inputs: &ExtensionInputs<'_>,
    vs: &VaultspeedHubSettings,
) -> Result<VsConfig> {
    let settings = inputs.settings;
    let gitlab = inputs
        .sign_in
        .then(|| {
            GitLab::resolve(GitLabSource {
                section: "[release_hub.vaultspeed]",
                repo_url: &vs.repo_url,
                gitlab_url: &vs.gitlab_url,
                fallback_gitlab_url: Some(&settings.gitlab_url),
                gitlab_project: &vs.gitlab_project,
                push_url: &vs.push_url,
            })
        })
        .transpose()?;
    Ok(VsConfig {
        repo_dir: ctx.cwd().join(&vs.repo_dir),
        repo_url: vs.repo_url.clone(),
        gitlab,
        release: ctx.config().section(VsReleaseSettings::SECTION)?,
        vaultspeed: ctx.config().section(VaultspeedSettings::SECTION)?,
        keep_jobs: settings.keep_jobs,
        git_timeout: (settings.git_timeout_secs > 0)
            .then(|| std::time::Duration::from_secs(settings.git_timeout_secs)),
        conflict_timeout: std::time::Duration::from_secs(settings.conflict_timeout_secs),
        state_dir: non_empty(&settings.state_dir).map(|d| PathBuf::from(d).join("vaultspeed")),
    })
}

/// The extension factory for [`vdp_release_ui_cli::hub::serve`]: the VaultSpeed extension, unless
/// `[release_hub.vaultspeed].enabled` is off. Segments other than VaultSpeed's are said, not ignored.
///
/// # Errors
///
/// An error when `[release_hub.vaultspeed]` is invalid, or the VaultSpeed hub configuration cannot be built (its
/// settings sections, or the GitLab sign-in settings).
pub fn extensions(
    ctx: &Context,
    inputs: &ExtensionInputs<'_>,
) -> Result<Vec<Arc<dyn HubExtension>>> {
    let settings = inputs.settings;
    for segment in settings.extensions.keys().filter(|s| *s != SEGMENT) {
        tracing::warn!(
            "[release_hub.{segment}] is set, but this binary serves no such release extension"
        );
    }
    let vs: VaultspeedHubSettings = settings.extension(SEGMENT).map_err(anyhow::Error::msg)?;
    if !vs.enabled {
        return Ok(Vec::new());
    }
    let config = build_config(ctx, inputs, &vs)?;
    Ok(vec![Arc::new(VaultspeedExtension::new(
        config,
        inputs.tracker.clone(),
    ))])
}

#[cfg(test)]
mod tests {
    use vdp_core::Config;
    use vdp_release_hub::HubSettings;

    use super::*;

    #[test]
    fn extensions_resolves_gitlab_url_override_and_hub_fallback() {
        let ctx = Context::builder(Config::default()).build();

        let mut settings = HubSettings {
            gitlab_url: "https://hub-fallback.example".into(),
            ..HubSettings::default()
        };
        settings.extensions.insert(
            SEGMENT.into(),
            serde_json::json!({
                "enabled": true,
                "repo_url": "git@git.example.com:vdp/vs.git",
                "gitlab_url": "https://vs-override.example"
            }),
        );
        let vs_override: VaultspeedHubSettings = settings.extension(SEGMENT).unwrap();
        let inputs = ExtensionInputs {
            settings: &settings,
            sign_in: true,
            tracker: None,
        };
        assert_eq!(extensions(&ctx, &inputs).unwrap().len(), 1);
        let cfg = build_config(&ctx, &inputs, &vs_override).unwrap();
        assert_eq!(
            cfg.gitlab.unwrap(),
            GitLab {
                url: "https://vs-override.example".into(),
                project: "vdp/vs".into(),
                push_url: "https://git.example.com/vdp/vs.git".into(),
            }
        );

        settings.extensions.insert(
            SEGMENT.into(),
            serde_json::json!({
                "enabled": true,
                "repo_url": "/tmp/vs.git",
                "gitlab_url": "",
                "gitlab_project": "vdp/vs",
                "push_url": "https://hub-fallback.example/vdp/vs.git"
            }),
        );
        let vs_fallback: VaultspeedHubSettings = settings.extension(SEGMENT).unwrap();
        let fallback_inputs = ExtensionInputs {
            settings: &settings,
            sign_in: true,
            tracker: None,
        };
        let fallback_cfg = build_config(&ctx, &fallback_inputs, &vs_fallback).unwrap();
        assert_eq!(
            fallback_cfg.gitlab.unwrap(),
            GitLab {
                url: "https://hub-fallback.example".into(),
                project: "vdp/vs".into(),
                push_url: "https://hub-fallback.example/vdp/vs.git".into(),
            }
        );

        let no_sign_in = ExtensionInputs {
            settings: &settings,
            sign_in: false,
            tracker: None,
        };
        assert_eq!(
            build_config(&ctx, &no_sign_in, &vs_fallback)
                .unwrap()
                .gitlab,
            None
        );
    }
}
