//! `release serve` with the VaultSpeed releases: the extension of the release hub that steers the framework
//! repository (`[release_hub.vaultspeed]`, `[vaultspeed_release]`, `[vaultspeed]`), when enabled.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use vdp_cli::commands::hub::{ExtensionInputs, extension_gitlab, non_empty};
use vdp_core::Context;
use vdp_release_hub::HubExtension;
use vdp_vaultspeed::VaultspeedSettings;
use vdp_vaultspeed_hub::{SEGMENT, VaultspeedExtension, VaultspeedHubSettings, VsConfig};
use vdp_vaultspeed_release::VsReleaseSettings;

/// The extension factory for [`vdp_cli::commands::hub::serve`]: the VaultSpeed extension, unless
/// `[release_hub.vaultspeed].enabled` is off. Segments other than VaultSpeed's are said, not ignored.
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
    let gitlab = extension_gitlab(
        inputs,
        "[release_hub.vaultspeed]",
        &vs.repo_url,
        &vs.gitlab_url,
        &vs.gitlab_project,
        &vs.push_url,
    )?;
    let config = VsConfig {
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
    };
    Ok(vec![Arc::new(VaultspeedExtension::new(
        config,
        inputs.tracker.clone(),
    ))])
}
