//! Command line of `vdp-vaultspeed`: `vdp`'s global options and commands, plus `vaultspeed`.

use std::path::PathBuf;

use clap::{ArgMatches, Args, FromArgMatches, Parser, Subcommand};
use vdp_cli::args::{Command as CoreCommand, GlobalArgs};

const VAULTSPEED: &str = "vaultspeed";

#[derive(Debug, Parser)]
#[command(
    name = "vdp-vaultspeed",
    version,
    about = "The vdp toolkit with the VaultSpeed Automation Framework: deployments, object history and VaultSpeed releases"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Debug logging (or set `VDP_LOG`).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// `dbt` output is the engine's own: the wrapper logs less.
    pub const fn is_wrapper(&self) -> bool {
        matches!(self.command, Command::Core(CoreCommand::Dbt { .. }))
    }
}

/// `vaultspeed`, or any command of `vdp`.
///
/// Written out instead of `#[command(flatten)]`: `vdp`'s command enum ends in an external subcommand (the hand-off
/// of `vdp vaultspeed ...`), and clap's derive for a flattened enum would route every subcommand name into
/// that fallback, including `vaultspeed`.
#[derive(Debug)]
pub enum Command {
    /// VaultSpeed deployments: PML scripts and VaultSpeed generations to SingleStore and Airflow.
    Vaultspeed(VaultspeedArgs),
    /// Every command of `vdp`.
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
        CoreCommand::augment_subcommands(command).subcommand(VaultspeedArgs::augment_args(
            clap::Command::new(VAULTSPEED).about(
                "VaultSpeed deployments: PML scripts and VaultSpeed generations to SingleStore and Airflow",
            ),
        ))
    }

    fn augment_subcommands_for_update(command: clap::Command) -> clap::Command {
        Self::augment_subcommands(command)
    }

    fn has_subcommand(name: &str) -> bool {
        name == VAULTSPEED || CoreCommand::has_subcommand(name)
    }
}

/// `vdp-vaultspeed vaultspeed ...`. The environment is `--env`, else `$TARGET_BRANCH` (set by the framework's
/// CI), else the usual `$DBT_DEPLOY_TARGET`/default.
#[derive(Debug, Args)]
pub struct VaultspeedArgs {
    /// The vaultspeed-automation-framework checkout (default: `[vaultspeed].project_dir`, the current directory).
    #[arg(long, global = true, env = "VDP_VAULTSPEED_DIR")]
    pub framework_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: VaultspeedCommand,
}

#[derive(Debug, Subcommand)]
pub enum VaultspeedCommand {
    /// Deploy everything staged: PML first, then generations in numeric order.
    Deploy(VaultspeedRunArgs),
    /// Show what `deploy` would do, changing nothing (same as `deploy --preview`).
    Plan(VaultspeedRunArgs),
    /// Ledger maintenance.
    #[command(subcommand)]
    Ledger(VaultspeedLedgerCommand),
    /// Rewrite-rule tooling.
    #[command(subcommand)]
    Rules(VaultspeedRulesCommand),
    /// DAG tooling.
    #[command(subcommand)]
    Dags(VaultspeedDagsCommand),
    /// Read the local object history (what happened to a table, view or procedure, across renames).
    History(VaultspeedHistoryArgs),
}

#[derive(Debug, Clone, Args)]
pub struct VaultspeedRunArgs {
    /// Use only the JSONL ledger and no database or Vault (implies preview).
    #[arg(long)]
    pub offline: bool,

    /// Only these generations (e.g. `1134_FMC 1133_DDL`); staged PML always runs.
    #[arg(long, num_args = 1..)]
    pub files: Option<Vec<String>>,

    /// Plan and render everything without changing anything.
    #[arg(long)]
    pub preview: bool,

    /// Print every group's decision and every DAG diff.
    #[arg(long)]
    pub explain: bool,

    /// Also write the full report (the plan, in preview) to this JSON file.
    #[arg(long)]
    pub report_out: Option<PathBuf>,

    /// Also write a JUnit XML report (for GitLab test reports).
    #[arg(long)]
    pub junit: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum VaultspeedLedgerCommand {
    /// Seed the ledger from the legacy `vaultspeed/deployed` files.
    Import {
        /// Use only the JSONL ledger and no database or Vault.
        #[arg(long)]
        offline: bool,
        /// Import even when the ledger already has entries.
        #[arg(long)]
        force: bool,
    },
    /// Summarise the ledger.
    Stats {
        /// Use only the JSONL ledger and no database or Vault.
        #[arg(long)]
        offline: bool,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RuleArtifact {
    Ddl,
    Etl,
}

#[derive(Debug, Subcommand)]
pub enum VaultspeedRulesCommand {
    /// Show how the pipeline splits, rewrites and classifies a SQL file.
    Test {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "ddl")]
        artifact: RuleArtifact,
    },
}

#[derive(Debug, Subcommand)]
pub enum VaultspeedDagsCommand {
    /// Render the DAGs of an FMC zip, a directory or a `.py` file and print the diffs.
    Render {
        path: PathBuf,
        /// SingleStore host embedded in dataset URIs (defaults to `SINGLESTORE_HOST`).
        #[arg(long, env = "SINGLESTORE_HOST")]
        host: Option<String>,
    },
}

/// `vaultspeed history ...`: reads the DuckDB file of the selected environment's objects.
#[derive(Debug, Args)]
pub struct VaultspeedHistoryArgs {
    /// The history file (default: `[vaultspeed.history].path` in the framework checkout).
    #[arg(long, global = true)]
    pub path: Option<PathBuf>,

    /// Print JSON instead of a table.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: VaultspeedHistoryCommand,
}

#[derive(Debug, Subcommand)]
pub enum VaultspeedHistoryCommand {
    /// The statements that changed an object, oldest first: the object currently (or last) named `identifier`,
    /// followed across renames.
    Object {
        /// `database.name`.
        identifier: String,
        /// List every lifespan that used the name (a name can be reused after a rename or a drop).
        #[arg(long)]
        all: bool,
    },
    /// The objects of the environment.
    Objects {
        /// Only live or only dropped objects.
        #[arg(long, value_enum)]
        status: Option<StatusFilter>,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum StatusFilter {
    Live,
    Dropped,
}
