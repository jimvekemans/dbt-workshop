//! Command line of `vdp-release-ui`: `vdp`'s global options and commands, plus `release serve`.

use std::path::PathBuf;

use clap::{ArgMatches, Args, FromArgMatches, Parser, Subcommand};
use vdp_cli::args::{Command as CoreCommand, GlobalArgs};

use crate::help;

const RELEASE: &str = "release";
const SERVE: &str = "serve";

/// The `vdp-release-ui` command line: the [`GlobalArgs`] every command shares, and one [`Command`].
#[derive(Debug, Parser)]
#[command(
    name = "vdp-release-ui",
    version,
    about = "The vdp toolkit plus `release serve`, the release hub behind the release UI",
    long_about = help::ROOT_LONG_ABOUT,
    after_long_help = help::ROOT_AFTER_LONG_HELP
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// `dbt` output is the engine's own: the wrapper logs less.
    #[must_use]
    pub const fn is_wrapper(&self) -> bool {
        self.command.is_wrapper()
    }
}

/// `release serve`, or any command of `vdp`.
///
/// Written out instead of `#[command(flatten)]`: `vdp`'s command enum ends in an external subcommand (the hand-off
/// of `vdp vaultspeed ...` and `vdp release-ui ...`), and clap's derive for a flattened enum would route every
/// subcommand name into that fallback. `serve` sits below `release`, next to `vdp`'s own release commands
/// (`create`, `analyse`, `patch`, `promote`, `branches`), which `vdp release ...` and this binary share.
#[derive(Debug)]
pub enum Command {
    /// `release serve`: the release hub.
    ReleaseServe(ServeArgs),
    /// Every command of `vdp`.
    Core(CoreCommand),
}

impl Command {
    /// Whether the command is `dbt`, the wrapper around the dbt engine.
    #[must_use]
    pub const fn is_wrapper(&self) -> bool {
        matches!(self, Self::Core(CoreCommand::Dbt { .. }))
    }
}

impl FromArgMatches for Command {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        Self::from_arg_matches_mut(&mut matches.clone())
    }

    fn from_arg_matches_mut(matches: &mut ArgMatches) -> Result<Self, clap::Error> {
        let serves = matches.subcommand().is_some_and(|(name, release)| {
            name == RELEASE && release.subcommand_name() == Some(SERVE)
        });
        if serves
            && let Some((_, mut release)) = matches.remove_subcommand()
            && let Some((_, mut serve)) = release.remove_subcommand()
        {
            return ServeArgs::from_arg_matches_mut(&mut serve).map(Self::ReleaseServe);
        }
        CoreCommand::from_arg_matches_mut(matches).map(Self::Core)
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Subcommand for Command {
    fn augment_subcommands(command: clap::Command) -> clap::Command {
        CoreCommand::augment_subcommands(command).mut_subcommand(RELEASE, |release| {
            release.subcommand(ServeArgs::augment_args(clap::Command::new(SERVE)))
        })
    }

    fn augment_subcommands_for_update(command: clap::Command) -> clap::Command {
        Self::augment_subcommands(command)
    }

    fn has_subcommand(name: &str) -> bool {
        CoreCommand::has_subcommand(name)
    }
}

// The doc comment of `ServeArgs` is the command's `--help` text, as for every clap command: it stays one paragraph.
// Every option defaults to its `[release_hub]` setting.
/// Serve the release hub: the HTTP API, and optionally the built UI
#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(long_about = help::SERVE_LONG_ABOUT, after_long_help = help::SERVE_AFTER_LONG_HELP)]
pub struct ServeArgs {
    /// The clone (default: `[release_hub] repo_dir`)
    #[arg(long, value_name = "DIR", long_help = help::SERVE_REPO_DIR_LONG)]
    pub repo_dir: Option<PathBuf>,
    /// Address to listen on (default: `[release_hub] bind`)
    #[arg(long, value_name = "ADDRESS", long_help = help::SERVE_BIND_LONG)]
    pub bind: Option<String>,
    /// Port (default: `[release_hub] port`)
    #[arg(long, value_name = "PORT", long_help = help::SERVE_PORT_LONG)]
    pub port: Option<u16>,
    /// The built frontend to serve next to the API
    #[arg(long, value_name = "DIR", long_help = help::SERVE_UI_DIR_LONG)]
    pub ui_dir: Option<PathBuf>,
    /// Allow a frontend on this origin (e.g. http://localhost:5173)
    #[arg(long, value_name = "ORIGIN", long_help = help::SERVE_CORS_ORIGIN_LONG)]
    pub cors_origin: Option<String>,
    /// Validate releases with dbt (`[release] validate`) before pushing
    #[arg(long, long_help = help::SERVE_VALIDATE_LONG)]
    pub validate: bool,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;
    use vdp_cli::args::ReleaseCommand;

    use super::*;

    #[test]
    fn release_serve_parses_with_its_options_and_global_options_anywhere() {
        let cli = Cli::try_parse_from([
            "vdp-release-ui",
            "-q",
            "release",
            "serve",
            "--bind",
            "127.0.0.1",
            "--port",
            "0",
            "--ui-dir",
            "/tmp/ui",
            "--validate",
            "--no-vault",
        ])
        .unwrap();
        assert!(cli.global.quiet);
        assert!(cli.global.no_vault);
        assert!(!cli.is_wrapper());
        let Command::ReleaseServe(args) = cli.command else {
            panic!("expected release serve");
        };
        assert_eq!(args.bind.as_deref(), Some("127.0.0.1"));
        assert_eq!(args.port, Some(0));
        assert_eq!(args.ui_dir, Some(PathBuf::from("/tmp/ui")));
        assert!(args.validate);
        assert_eq!(args.repo_dir, None);
        assert_eq!(args.cors_origin, None);
    }

    #[test]
    fn the_release_commands_of_vdp_still_parse() {
        let cli = Cli::try_parse_from(["vdp-release-ui", "release", "analyse", "--json"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Core(CoreCommand::Release(ReleaseCommand::Analyse {
                json: true,
                ..
            }))
        ));
        let cli = Cli::try_parse_from(["vdp-release-ui", "release", "branches"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Core(CoreCommand::Release(ReleaseCommand::Branches))
        ));
    }

    #[test]
    fn every_other_command_of_vdp_is_accepted() {
        let cli = Cli::try_parse_from(["vdp-release-ui", "env", "show"]).unwrap();
        assert!(matches!(cli.command, Command::Core(CoreCommand::Env(_))));
        let cli = Cli::try_parse_from(["vdp-release-ui", "dbt", "run"]).unwrap();
        assert!(cli.is_wrapper());
    }

    #[test]
    fn serve_is_listed_below_release_and_nothing_else_changes() {
        let mut command = Cli::command();
        let release = command.find_subcommand_mut(RELEASE).unwrap();
        let names: Vec<&str> = release
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect();
        assert!(names.contains(&SERVE), "{names:?}");
        for own in ["create", "analyse", "patch", "promote", "branches"] {
            assert!(names.contains(&own), "{own} missing from {names:?}");
        }
        // Only below `release`: elsewhere `serve` is an unknown command, which `vdp` leaves to its hand-off.
        let cli = Cli::try_parse_from(["vdp-release-ui", "serve"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Core(CoreCommand::External(_))
        ));
    }
    /// Every command of the built command tree, with the path that names it in a failure.
    fn walk<'a>(
        command: &'a clap::Command,
        path: &str,
        visit: &mut impl FnMut(&str, &'a clap::Command),
    ) {
        visit(path, command);
        for sub in command.get_subcommands() {
            if sub.is_hide_set() || sub.get_name() == "help" {
                continue;
            }
            walk(sub, &format!("{path} {}", sub.get_name()), visit);
        }
    }

    fn built() -> clap::Command {
        let mut command = Cli::command();
        command.build();
        command
    }

    #[test]
    fn the_command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_and_option_has_help_text() {
        let root = built();
        let mut problems = std::collections::BTreeSet::new();
        walk(&root, "vdp-release-ui", &mut |path, command| {
            if command.get_about().is_none() {
                problems.insert(format!("`{path}` has no about"));
            }
            if command.get_long_about().is_none() {
                problems.insert(format!("`{path}` has no long_about"));
            }
            let leaf = !command.has_subcommands();
            let examples = command
                .get_after_long_help()
                .is_some_and(|text| text.to_string().contains("Examples:"));
            if leaf && !examples {
                problems.insert(format!("`{path}` has no \"Examples:\" in after_long_help"));
            }
            for arg in command.get_arguments() {
                let builtin = matches!(arg.get_id().as_str(), "help" | "version");
                if !arg.is_hide_set() && !builtin && arg.get_help().is_none() {
                    problems.insert(format!("`{path}`: `{}` has no help", arg.get_id()));
                }
            }
        });
        assert!(
            problems.is_empty(),
            "help text is missing:\n{}",
            problems.into_iter().collect::<Vec<_>>().join("\n")
        );
    }

    #[test]
    fn help_lines_stay_within_100_columns() {
        let root = built();
        let mut wide = Vec::new();
        walk(&root, "vdp-release-ui", &mut |path, command| {
            for (kind, text) in [
                ("-h", command.clone().render_help().to_string()),
                ("--help", command.clone().render_long_help().to_string()),
            ] {
                for line in text.lines().filter(|l| l.chars().count() > 100) {
                    wide.push(format!("`{path} {kind}`: {line}"));
                }
            }
        });
        assert!(
            wide.is_empty(),
            "lines over 100 columns:\n{}",
            wide.join("\n")
        );
    }
}
