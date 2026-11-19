use std::process::ExitCode;

use clap::{CommandFactory as _, Parser as _};
use vdp_cli::args::Command as Core;
use vdp_cli::{bootstrap, runtime};
use vdp_core::Outcome;
use vdp_release_ui_cli::args::Command as UiCommand;
use vdp_vaultspeed_cli::args::{Cli, Command};
use vdp_vaultspeed_cli::commands::vaultspeed;
use vdp_vaultspeed_cli::hub;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let global = cli.global.clone();
    let is_wrapper = cli.is_wrapper();
    runtime::run(
        runtime::RunContext {
            program: "vdp-vaultspeed",
            global: &global,
            is_wrapper,
        },
        || run(cli),
    )
}

fn run(cli: Cli) -> anyhow::Result<Outcome> {
    let Cli {
        mut global,
        command,
        ..
    } = cli;
    match command {
        Command::Vaultspeed(args) => {
            // `$TARGET_BRANCH` (the framework's CI) names the environment when `--env` does not.
            global.environment = vaultspeed::environment_override(&global)?;
            let ctx = bootstrap::context(&global, &std::env::current_dir()?)?;
            vaultspeed::run(&ctx, &global, &args)
        }
        // `vdp`'s hand-offs to other binaries (`vdp vaultspeed ...`) are not commands here.
        Command::Core(UiCommand::Core(Core::External(external))) => Cli::command()
            .error(
                clap::error::ErrorKind::InvalidSubcommand,
                format!(
                    "unrecognized subcommand '{}'",
                    external
                        .first()
                        .map_or_else(String::new, |a| a.to_string_lossy().into_owned())
                ),
            )
            .exit(),
        // `release serve` gets the VaultSpeed extension; every other command is `vdp`'s.
        Command::Core(command) => {
            vdp_release_ui_cli::run_command(&global, command, &hub::extensions)
        }
    }
}
