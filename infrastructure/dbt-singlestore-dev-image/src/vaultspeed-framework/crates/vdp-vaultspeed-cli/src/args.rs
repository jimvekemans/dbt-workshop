//! Command line of `vdp-vaultspeed`: `vdp`'s global options and commands, plus `vaultspeed`.

use std::path::PathBuf;

use clap::{ArgMatches, Args, FromArgMatches, Parser, Subcommand};
use vdp_cli::args::GlobalArgs;
use vdp_release_ui_cli::args::Command as CoreCommand;

use crate::help;

const VAULTSPEED: &str = "vaultspeed";

/// The `vdp-vaultspeed` command line: the [`GlobalArgs`] every command shares, and one [`Command`].
#[derive(Debug, Parser)]
#[command(
    name = "vdp-vaultspeed",
    version,
    about = "The vdp toolkit plus `vaultspeed`: VaultSpeed deployments, history and releases",
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
    pub const fn is_wrapper(&self) -> bool {
        matches!(&self.command, Command::Core(core) if core.is_wrapper())
    }
}

/// `vaultspeed`, or any command of `vdp-release-ui` (`vdp`'s, and `release serve`).
///
/// Written out instead of `#[command(flatten)]`: `vdp`'s command enum ends in an external subcommand (the hand-off
/// of `vdp vaultspeed ...`), and clap's derive for a flattened enum would route every subcommand name into
/// that fallback, including `vaultspeed`.
#[derive(Debug)]
pub enum Command {
    /// VaultSpeed deployments: PML and generations to SingleStore and Airflow
    Vaultspeed(VaultspeedArgs),
    /// Every command of `vdp-release-ui`: those of `vdp`, and `release serve` (which gets the VaultSpeed extension).
    Core(CoreCommand),
}

impl FromArgMatches for Command {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        Self::from_arg_matches_mut(&mut matches.clone())
    }

    fn from_arg_matches_mut(matches: &mut ArgMatches) -> Result<Self, clap::Error> {
        if matches.subcommand_name() == Some(VAULTSPEED)
            && let Some((_, mut sub)) = matches.remove_subcommand()
        {
            return VaultspeedArgs::from_arg_matches_mut(&mut sub).map(Self::Vaultspeed);
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
        CoreCommand::augment_subcommands(command)
            .mut_subcommand("release", |release| {
                release.mut_subcommand("serve", |serve| {
                    // Here the hub also serves the VaultSpeed releases.
                    let long = serve
                        .get_long_about()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    serve
                        .long_about(format!("{long}\n\n{}", help::SERVE_EXTENSION_NOTE))
                        .after_long_help(help::SERVE_AFTER_LONG_HELP)
                })
            })
            .subcommand(VaultspeedArgs::augment_args(clap::Command::new(VAULTSPEED)))
    }

    fn augment_subcommands_for_update(command: clap::Command) -> clap::Command {
        Self::augment_subcommands(command)
    }

    fn has_subcommand(name: &str) -> bool {
        name == VAULTSPEED || CoreCommand::has_subcommand(name)
    }
}

// `vdp-vaultspeed vaultspeed ...`. The environment is `--env`, else `$TARGET_BRANCH` (set by the framework's
// CI), else the usual `$DBT_DEPLOY_TARGET`/default. A plain comment, not a doc comment: the doc comment of an
// `Args` struct is the command's `about`, and it would replace the one set here.
#[derive(Debug, Args)]
#[command(
    about = help::VAULTSPEED_ABOUT,
    long_about = help::VAULTSPEED_LONG_ABOUT
)]
pub struct VaultspeedArgs {
    /// The framework checkout directory
    #[arg(
        long,
        global = true,
        env = "VDP_VAULTSPEED_DIR",
        value_name = "DIR",
        long_help = help::FRAMEWORK_DIR_LONG
    )]
    pub framework_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: VaultspeedCommand,
}

/// `vdp-vaultspeed vaultspeed ...`: what the VaultSpeed module adds.
#[derive(Debug, Subcommand)]
pub enum VaultspeedCommand {
    /// Deploy everything staged: PML first, then generations in numeric order
    #[command(
        long_about = help::DEPLOY_LONG_ABOUT,
        after_long_help = help::DEPLOY_AFTER_LONG_HELP
    )]
    Deploy(VaultspeedRunArgs),
    /// Show what `deploy` would do, changing nothing (same as `deploy --preview`)
    #[command(
        long_about = help::PLAN_LONG_ABOUT,
        after_long_help = help::PLAN_AFTER_LONG_HELP
    )]
    Plan(VaultspeedRunArgs),
    /// Ledger maintenance
    #[command(subcommand, long_about = help::LEDGER_LONG_ABOUT)]
    Ledger(VaultspeedLedgerCommand),
    /// Rewrite-rule tooling
    #[command(subcommand, long_about = help::RULES_LONG_ABOUT)]
    Rules(VaultspeedRulesCommand),
    /// DAG tooling
    #[command(subcommand, long_about = help::DAGS_LONG_ABOUT)]
    Dags(VaultspeedDagsCommand),
    /// Read the local object history of tables, views and procedures
    #[command(long_about = help::HISTORY_LONG_ABOUT)]
    History(VaultspeedHistoryArgs),
}

/// The options of `deploy` and `plan`.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct VaultspeedRunArgs {
    /// Use only the JSONL ledger and no database or Vault (needs --preview)
    #[arg(long, long_help = help::RUN_OFFLINE_LONG)]
    pub offline: bool,

    /// Only these generations; staged PML always runs
    #[arg(long, num_args = 1.., value_name = "GENERATION", long_help = help::RUN_FILES_LONG)]
    pub files: Option<Vec<String>>,

    /// Plan and render without changing anything
    #[arg(long, visible_alias = "dry-run", long_help = help::RUN_PREVIEW_LONG)]
    pub preview: bool,

    /// Print every group's decision and every DAG diff
    #[arg(long, long_help = help::RUN_EXPLAIN_LONG)]
    pub explain: bool,

    /// Also write the full report (the plan, in preview) to this JSON file
    #[arg(long, value_name = "FILE", long_help = help::RUN_REPORT_OUT_LONG)]
    pub report_out: Option<PathBuf>,

    /// Also write a JUnit XML report (for GitLab test reports)
    #[arg(long, value_name = "FILE", long_help = help::RUN_JUNIT_LONG)]
    pub junit: Option<PathBuf>,
}

/// `vaultspeed ledger ...`: ledger maintenance.
#[derive(Debug, Subcommand)]
pub enum VaultspeedLedgerCommand {
    /// Seed the ledger from the legacy `vaultspeed/deployed` files
    #[command(
        long_about = help::LEDGER_IMPORT_LONG_ABOUT,
        after_long_help = help::LEDGER_IMPORT_AFTER_LONG_HELP
    )]
    Import {
        /// Use only the JSONL ledger and no database or Vault
        #[arg(long, long_help = help::LEDGER_IMPORT_OFFLINE_LONG)]
        offline: bool,
        /// Import even when the ledger already has entries
        #[arg(long, long_help = help::LEDGER_IMPORT_FORCE_LONG)]
        force: bool,
    },
    /// Summarise the ledger
    #[command(
        long_about = help::LEDGER_STATS_LONG_ABOUT,
        after_long_help = help::LEDGER_STATS_AFTER_LONG_HELP
    )]
    Stats {
        /// Use only the JSONL ledger and no database or Vault
        #[arg(long, long_help = help::LEDGER_STATS_OFFLINE_LONG)]
        offline: bool,
    },
}

/// Which kind of artifact the rewrite rules are tried on.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RuleArtifact {
    /// Data definition: split, comments stripped (unless SHARD KEY hints), DDL rules
    Ddl,
    /// Load procedures and views: split, ETL rules
    Etl,
}

/// `vaultspeed rules ...`: rewrite-rule tooling.
#[derive(Debug, Subcommand)]
pub enum VaultspeedRulesCommand {
    /// Show how the pipeline splits, rewrites and classifies a SQL file
    #[command(
        long_about = help::RULES_TEST_LONG_ABOUT,
        after_long_help = help::RULES_TEST_AFTER_LONG_HELP
    )]
    Test {
        /// The SQL file to run through the pipeline
        #[arg(value_name = "FILE", long_help = help::RULES_TEST_FILE_LONG)]
        file: PathBuf,
        /// Pipeline and rules to use
        #[arg(
            long,
            value_enum,
            default_value = "ddl",
            long_help = help::RULES_TEST_ARTIFACT_LONG
        )]
        artifact: RuleArtifact,
    },
}

/// `vaultspeed dags ...`: DAG tooling.
#[derive(Debug, Subcommand)]
pub enum VaultspeedDagsCommand {
    /// Render the DAGs of an FMC zip, a directory or a `.py` file and print the diffs
    #[command(
        long_about = help::DAGS_RENDER_LONG_ABOUT,
        after_long_help = help::DAGS_RENDER_AFTER_LONG_HELP
    )]
    Render {
        /// An FMC zip, a directory or a `.py` file
        #[arg(value_name = "PATH", long_help = help::DAGS_RENDER_PATH_LONG)]
        path: PathBuf,
        /// SingleStore host for the dataset URIs
        #[arg(
            long,
            env = "SINGLESTORE_HOST",
            value_name = "HOST",
            long_help = help::DAGS_RENDER_HOST_LONG
        )]
        host: Option<String>,
    },
}

/// `vaultspeed history ...`: reads the DuckDB file of the selected environment's objects.
#[derive(Debug, Args)]
pub struct VaultspeedHistoryArgs {
    /// The history file to read
    #[arg(long, global = true, value_name = "FILE", long_help = help::HISTORY_PATH_LONG)]
    pub path: Option<PathBuf>,

    /// Print JSON instead of a table
    #[arg(long, global = true, long_help = help::HISTORY_JSON_LONG)]
    pub json: bool,

    #[command(subcommand)]
    pub command: VaultspeedHistoryCommand,
}

/// `vaultspeed history ...`: reading the object history.
#[derive(Debug, Subcommand)]
pub enum VaultspeedHistoryCommand {
    /// The statements that changed an object, oldest first
    #[command(
        long_about = help::HISTORY_OBJECT_LONG_ABOUT,
        after_long_help = help::HISTORY_OBJECT_AFTER_LONG_HELP
    )]
    Object {
        /// `database.name`
        #[arg(value_name = "IDENTIFIER", long_help = help::HISTORY_OBJECT_IDENTIFIER_LONG)]
        identifier: String,
        /// List every lifespan that used the name
        #[arg(long, long_help = help::HISTORY_OBJECT_ALL_LONG)]
        all: bool,
    },
    /// The objects of the environment
    #[command(
        long_about = help::HISTORY_OBJECTS_LONG_ABOUT,
        after_long_help = help::HISTORY_OBJECTS_AFTER_LONG_HELP
    )]
    Objects {
        /// Only live or only dropped objects
        #[arg(long, value_enum, long_help = help::HISTORY_OBJECTS_STATUS_LONG)]
        status: Option<StatusFilter>,
    },
}

/// Which objects `history objects` lists: only live or only dropped ones.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum StatusFilter {
    /// Objects that exist
    Live,
    /// Objects that were dropped
    Dropped,
}

#[cfg(test)]
mod tests {
    use vdp_core::Environment;

    use super::*;

    #[test]
    fn parses_existing_and_new_global_flags_before_and_after_subcommand() {
        let vs_before = Cli::try_parse_from([
            "vdp-vaultspeed",
            "--env",
            "ABT",
            "--no-vault",
            "-v",
            "--log-json",
            "/tmp/vs.json",
            "vaultspeed",
            "plan",
            "--preview",
        ])
        .unwrap();
        assert_eq!(vs_before.global.environment, Some(Environment::ABT));
        assert!(vs_before.global.no_vault);
        assert!(vs_before.global.verbose);
        assert_eq!(
            vs_before.global.log_json,
            Some(PathBuf::from("/tmp/vs.json"))
        );
        assert!(matches!(vs_before.command, Command::Vaultspeed(_)));

        let vs_after = Cli::try_parse_from([
            "vdp-vaultspeed",
            "vaultspeed",
            "--framework-dir",
            "/tmp/fw",
            "plan",
            "-q",
            "--log-json",
            "/tmp/after.json",
            "--env",
            "CBT",
        ])
        .unwrap();
        assert_eq!(vs_after.global.environment, Some(Environment::CBT));
        assert!(vs_after.global.quiet);
        assert_eq!(
            vs_after.global.log_json,
            Some(PathBuf::from("/tmp/after.json"))
        );
        let Command::Vaultspeed(vs) = vs_after.command else {
            panic!("expected Vaultspeed subcommand");
        };
        assert_eq!(vs.framework_dir, Some(PathBuf::from("/tmp/fw")));

        let core_after = Cli::try_parse_from([
            "vdp-vaultspeed",
            "env",
            "show",
            "-v",
            "--log-json",
            "/tmp/core.json",
        ])
        .unwrap();
        assert!(core_after.global.verbose);
        assert_eq!(
            core_after.global.log_json,
            Some(PathBuf::from("/tmp/core.json"))
        );
        assert!(matches!(core_after.command, Command::Core(_)));
    }

    #[test]
    fn runtime_exit_maps_outcomes_for_vaultspeed_binary() {
        assert_eq!(
            vdp_cli::runtime::exit_code("vdp-vaultspeed", Ok(vdp_core::Outcome::Success)),
            0
        );
        assert_eq!(
            vdp_cli::runtime::exit_code("vdp-vaultspeed", Ok(vdp_core::Outcome::Failures)),
            1
        );
        assert_eq!(
            vdp_cli::runtime::exit_code("vdp-vaultspeed", Err(anyhow::anyhow!("boom"))),
            2
        );
    }

    #[test]
    fn deploy_dry_run_is_alias_of_preview_and_shown_in_help() {
        use clap::CommandFactory;

        let dry_run = Cli::try_parse_from(["vdp-vaultspeed", "vaultspeed", "deploy", "--dry-run"])
            .expect("--dry-run parses");
        let preview = Cli::try_parse_from(["vdp-vaultspeed", "vaultspeed", "deploy", "--preview"])
            .expect("--preview parses");

        let Command::Vaultspeed(VaultspeedArgs {
            command: VaultspeedCommand::Deploy(dry_run_args),
            ..
        }) = dry_run.command
        else {
            panic!("expected vaultspeed deploy command");
        };
        let Command::Vaultspeed(VaultspeedArgs {
            command: VaultspeedCommand::Deploy(preview_args),
            ..
        }) = preview.command
        else {
            panic!("expected vaultspeed deploy command");
        };
        assert!(dry_run_args.preview);
        assert_eq!(dry_run_args, preview_args);

        let mut cmd = Cli::command();
        let deploy = cmd
            .find_subcommand_mut("vaultspeed")
            .expect("vaultspeed subcommand")
            .find_subcommand_mut("deploy")
            .expect("deploy subcommand");
        let help = deploy.render_long_help().to_string();
        assert!(
            help.contains("--preview"),
            "help missing --preview:\n{help}"
        );
        assert!(
            help.contains("[alias: --dry-run]"),
            "help missing visible alias:\n{help}"
        );
    }

    #[test]
    fn discovery_error_renders_path_and_cause_once_under_alternate() {
        let discovery =
            vdp_vaultspeed::engine::EngineError::from(vdp_vaultspeed::sources::SourceError::Io {
                path: PathBuf::from("/restricted/01_init.sql"),
                source: std::io::Error::from_raw_os_error(13),
            });
        assert_eq!(
            format!("{:#}", anyhow::Error::new(discovery)),
            "source discovery failed: /restricted/01_init.sql: Permission denied (os error 13)"
        );
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
        use clap::CommandFactory as _;

        let mut command = Cli::command();
        command.build();
        command
    }

    #[test]
    fn the_command_tree_is_valid() {
        use clap::CommandFactory as _;

        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_and_option_has_help_text() {
        let root = built();
        let mut problems = std::collections::BTreeSet::new();
        walk(&root, "vdp-vaultspeed", &mut |path, command| {
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
        walk(&root, "vdp-vaultspeed", &mut |path, command| {
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

    #[test]
    fn the_vaultspeed_command_keeps_its_own_about() {
        let root = built();
        let vaultspeed = root
            .find_subcommand(VAULTSPEED)
            .expect("vaultspeed command");
        let about = vaultspeed.get_about().expect("about").to_string();
        assert!(about.starts_with("VaultSpeed deployments"), "{about}");
        let long = vaultspeed.get_long_about().expect("long_about").to_string();
        assert!(long.contains("$TARGET_BRANCH"), "{long}");
    }
}
