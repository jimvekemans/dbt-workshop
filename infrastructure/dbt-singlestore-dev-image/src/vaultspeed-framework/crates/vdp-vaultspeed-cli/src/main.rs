use std::process::ExitCode;

use clap::{CommandFactory as _, Parser as _};
use vdp_cli::args::{Command as Core, ReleaseCommand};
use vdp_cli::{bootstrap, commands as core_commands};
use vdp_core::Outcome;
use vdp_vaultspeed_cli::args::{Cli, Command};
use vdp_vaultspeed_cli::commands::vaultspeed;
use vdp_vaultspeed_cli::hub;

fn main() -> ExitCode {
    let cli = Cli::parse();
    bootstrap::init_logging(cli.verbose, if cli.is_wrapper() { "warn" } else { "info" });
    match run(cli) {
        Ok(outcome) => ExitCode::from(outcome.exit_code()),
        Err(error) => {
            eprintln!("vdp-vaultspeed: error: {error:#}");
            ExitCode::from(2)
        }
    }
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
        // The hub gets the VaultSpeed extension; every other command is `vdp`'s.
        Command::Core(Core::Release(command @ ReleaseCommand::Serve { .. })) => {
            let ctx = bootstrap::context(&global, &std::env::current_dir()?)?;
            core_commands::hub::serve(&ctx, &command, &hub::extensions)
        }
        Command::Core(Core::External(external)) => Cli::command()
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
        Command::Core(command) => core_commands::run_command(&global, command),
    }
}
