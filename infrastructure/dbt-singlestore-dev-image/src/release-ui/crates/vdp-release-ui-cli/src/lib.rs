//! `vdp-release-ui`: the `vdp` command line (as a library, `vdp-cli`) plus `release serve`, the backend of the
//! release UI.
//!
//! The release UI's composition root: it parses `vdp`'s global options and commands, adds `release serve` below
//! `release`, and wires the release hub ([`vdp_release_hub`]) to the services of [`vdp_cli::bootstrap::context`]
//! (Jira, the secret chain, the dbt engine, SMTP through [`vdp_notify`]). The library holds what the `vdp-release-ui`
//! binary runs; another binary that serves the hub with more repositories (`vdp-vaultspeed`) builds on it.
//!
//! # Responsibilities
//!
//! - [`args`]: [`args::Cli`] and [`args::Command`] (`release serve`, or any command of `vdp`), and the options of
//!   `release serve` ([`args::ServeArgs`]).
//! - [`hub`]: [`hub::serve`] starts the hub with the extensions an [`hub::ExtensionFactory`] builds;
//!   [`hub::no_extensions`] is the factory of `vdp-release-ui` itself.
//! - [`run_command`]: runs a [`args::Command`] for a binary that supplies its own extension factory.
//!
//! Every command other than `release serve` is `vdp`'s own and runs through [`vdp_cli::commands::run_command`].
//!
//! # Example
//!
//! ```
//! use clap::Parser;
//! use vdp_release_ui_cli::args::{Cli, Command};
//!
//! let cli = Cli::try_parse_from(["vdp-release-ui", "release", "serve", "--port", "8765"])?;
//! assert!(matches!(cli.command, Command::ReleaseServe(ref serve) if serve.port == Some(8765)));
//! // Every command of `vdp` is accepted too, the release flows included.
//! let cli = Cli::try_parse_from(["vdp-release-ui", "release", "analyse", "--json"])?;
//! assert!(matches!(cli.command, Command::Core(_)));
//! # Ok::<(), clap::Error>(())
//! ```

pub mod args;
mod help;
pub mod hub;

use anyhow::Result;
use vdp_cli::args::GlobalArgs;
use vdp_core::Outcome;

use crate::args::Command;
use crate::hub::ExtensionFactory;

/// Runs `command` with the toolkit's own services: `release serve` starts the hub with the extensions
/// `extensions` builds, every other command is `vdp`'s ([`vdp_cli::commands::run_command`]).
///
/// # Errors
///
/// An error when the working directory or the context cannot be set up, or the command fails (see [`hub::serve`]).
pub fn run_command(
    global: &GlobalArgs,
    command: Command,
    extensions: ExtensionFactory<'_>,
) -> Result<Outcome> {
    match command {
        Command::ReleaseServe(args) => {
            let ctx = vdp_cli::bootstrap::context(global, &std::env::current_dir()?)?;
            hub::serve(&ctx, &args, extensions)
        }
        Command::Core(command) => vdp_cli::commands::run_command(global, command),
    }
}
