//! `vdp airflow ...`

use std::sync::Arc;

use anyhow::Result;
use vdp_airflow::AirflowSettings;
use vdp_core::{Context, Outcome};
use vdp_secrets::SecretProvider;

use crate::args::AirflowCommand;

pub(super) fn airflow(ctx: &Context, command: &AirflowCommand) -> Result<Outcome> {
    let AirflowCommand::Sync { tag } = command;
    let settings: AirflowSettings = ctx.config().section(AirflowSettings::SECTION)?;
    let secrets = ctx.service::<Arc<dyn SecretProvider>>()?;
    let (shell, _key) = vdp_airflow::connect(&settings, secrets.as_ref())?;
    let output = vdp_airflow::sync(&shell, &settings, tag)?;
    for line in output.stdout.lines().chain(output.stderr.lines()) {
        println!("{line}");
    }
    println!(
        "Airflow project at {} is on {tag}",
        settings.remote_project_dir
    );
    Ok(Outcome::Success)
}
