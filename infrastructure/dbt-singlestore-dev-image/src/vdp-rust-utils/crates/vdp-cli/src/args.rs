//! Command-line interface of `vdp`.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use vdp_core::Environment;

#[derive(Debug, Parser)]
#[command(
    name = "vdp",
    version,
    about = "VDP toolkit for dbt v2 on SingleStore: the dbt wrapper, source maintenance, releases and more"
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

/// Options every command shares.
#[derive(Debug, Clone, Default, Args)]
pub struct GlobalArgs {
    /// Environment: LDV, ABT, CBT, PRD or one configured as `[environments.<name>]` (default: `$DBT_DEPLOY_TARGET`,
    /// else `[core].default_environment`).
    #[arg(long = "env", global = true, value_parser = parse_environment)]
    pub environment: Option<Environment>,

    /// dbt project directory (default: the nearest parent with a `dbt_project.yml`).
    #[arg(long, global = true, env = "VDP_PROJECT_DIR")]
    pub project_dir: Option<PathBuf>,

    /// Extra configuration file, applied last (also `$VDP_CONFIG`).
    #[arg(long, global = true, env = "VDP_CONFIG")]
    pub config: Option<PathBuf>,

    /// dbt target for commands that use the database or run dbt (default: the profile's target).
    #[arg(long, global = true)]
    pub target: Option<String>,

    /// Directory with profiles.yml (default: $DBT_PROFILES_DIR, the project, ~/.dbt).
    #[arg(long, global = true)]
    pub profiles_dir: Option<PathBuf>,

    /// Do not log in to Vault; secrets come from the dotenv file and the environment.
    #[arg(
        long,
        global = true,
        env = "VDP_NO_VAULT",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub no_vault: bool,
}

impl Cli {
    pub const fn is_wrapper(&self) -> bool {
        matches!(self.command, Command::Dbt { .. })
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run dbt through the wrapper (what the `dbt` link does): `vdp dbt build -s my_model`.
    #[command(disable_help_flag = true)]
    Dbt {
        /// Arguments for dbt, passed on unchanged.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<OsString>,
    },
    #[cfg(feature = "sources")]
    /// Maintain dbt sources from the SingleStore catalog.
    #[command(subcommand)]
    Sources(SourcesCommand),
    /// Metadata seeds.
    #[command(subcommand)]
    Seeds(SeedsCommand),
    /// Database maintenance report.
    #[command(subcommand)]
    Maintenance(MaintenanceCommand),
    #[cfg(feature = "release")]
    /// dbt DROP releases.
    #[command(subcommand)]
    Release(ReleaseCommand),
    #[cfg(feature = "release")]
    /// Jira tickets.
    #[command(subcommand)]
    Jira(JiraCommand),
    #[cfg(feature = "airflow")]
    /// The dbt project on the Airflow host.
    #[command(subcommand)]
    Airflow(AirflowCommand),
    #[cfg(feature = "sql")]
    /// Ad-hoc SQL against the dbt target's database.
    #[command(subcommand)]
    Sql(SqlCommand),
    #[cfg(feature = "qa")]
    /// Quality checks on the dbt manifest.
    #[command(subcommand)]
    Qa(QaCommand),
    /// Inspect the resolved environment.
    #[command(subcommand)]
    Env(EnvCommand),
    /// VaultSpeed deployments: PML scripts and VaultSpeed generations to SingleStore and Airflow.
    #[cfg(feature = "vaultspeed")]
    Vaultspeed(VaultspeedArgs),
    /// Maintenance of the toolkit itself.
    #[command(subcommand)]
    Dev(DevCommand),
}

#[cfg(feature = "sources")]
#[derive(Debug, Subcommand)]
pub enum SourcesCommand {
    /// Add tables to their sources, with their columns from the database.
    ///
    /// A source declared in a hand-written YAML file gets the table inserted there, leaving the rest of the
    /// file untouched; other sources live in `models/sources/sources_<database>.yml`.
    Add {
        /// Tables as database.table (or source.table for a declared source).
        tables: Vec<String>,
        /// Also add every source table dbt reports as undefined (runs `dbt parse`).
        #[arg(long)]
        missing: bool,
        /// Show what would change without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Regenerate the generated source files from the database and keep only referenced tables.
    Rewrite {
        /// Databases to regenerate (default: `[sources] databases`).
        #[arg(long, value_delimiter = ',')]
        databases: Option<Vec<String>>,
        /// Regenerate only; do not run dbt or trim.
        #[arg(long)]
        no_trim: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SeedsCommand {
    /// Rewrite seeds/meta/dbt_{models,sources}.{csv,yml} from the project (runs `dbt parse`).
    RefreshMetadata,
}

#[derive(Debug, Subcommand)]
pub enum MaintenanceCommand {
    /// Run the maintenance macro and save the JSON it prints.
    Export {
        /// Output file (default: `[maintenance] out`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[cfg(feature = "release")]
#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    /// Cut a release branch: revert tickets outside the target DROP, validate with dbt, push.
    Create {
        /// Release branch name, e.g. release/2026_DROP05.
        #[arg(long)]
        name: String,
        /// Target Jira fix version, e.g. 2026_DROP05.
        #[arg(long)]
        fixversion: String,
        /// Ref to cut the release from (default: `[release] from`, origin/dev).
        #[arg(long)]
        from: Option<String>,
        /// Base ref for the merge-base and for reverts to carry over (default: `[release] to`, origin/main).
        #[arg(long)]
        to: Option<String>,
        /// Keep tickets without a DROP fix version (they are reverted by default).
        #[arg(long)]
        keep_empty_fixversions: bool,
        /// Ask before each revert or cherry-pick.
        #[arg(long)]
        interactive: bool,
        /// Recreate the branch if it exists, and force-push (with lease).
        #[arg(long)]
        force: bool,
        /// Recreating drops commits only the old branch has (listed by --dry-run): accept that.
        #[arg(long)]
        accept_losses: bool,
        /// Do not commit `changelog/<fixversion>.md` as the branch's last commit.
        #[arg(long)]
        no_changelog: bool,
        /// Allow a release branch outside the newest two (or older than the newest).
        #[arg(long)]
        allow_old_release: bool,
        /// Analyse and print the plan; change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Show what a release (or with `--patch <branch>`, a patch) would contain.
    Analyse {
        /// Target Jira fix version, e.g. 2026_DROP05 (for a patch: default the branch suffix).
        #[arg(long)]
        fixversion: Option<String>,
        /// Source ref (default: `[release] from`).
        #[arg(long)]
        from: Option<String>,
        /// Compare ref (default: `[release] to`).
        #[arg(long)]
        to: Option<String>,
        /// Analyse a patch of this release branch instead.
        #[arg(long)]
        patch: Option<String>,
        /// Print the analysis as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Add tickets to an existing release branch (re-enable reverted ones, cherry-pick new ones) and push.
    Patch {
        /// The release branch, e.g. release/2026_DROP24.
        branch: String,
        /// Tickets to add (default: the analysis' defaults: emergency drops and re-enable candidates).
        #[arg(long, value_delimiter = ',')]
        include: Option<Vec<String>>,
        /// Target fix version (default: the branch suffix).
        #[arg(long)]
        fixversion: Option<String>,
        /// Source ref (default: `[release] from`).
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        interactive: bool,
        /// Allow patching a release branch outside the newest two.
        #[arg(long)]
        allow_old_release: bool,
        /// Do not rewrite the release changelog.
        #[arg(long)]
        no_changelog: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Bring a release branch to main: a new release resets main to the merge-base and merges; a patch merges
    /// onto main. Pushed with a lease on main as it was.
    Promote {
        /// The release branch.
        branch: String,
        /// `auto` decides by ancestry: a patch when main already has part of this release branch.
        #[arg(long, value_enum, default_value_t = PromoteKind::Auto)]
        kind: PromoteKind,
        /// Main has a newer release: promote this older one anyway (a rollback).
        #[arg(long)]
        accept_rollback: bool,
        /// A new promotion drops commits from main that the source lacks (listed by --dry-run): accept that.
        #[arg(long)]
        accept_losses: bool,
        /// Allow promoting a release branch outside the newest two.
        #[arg(long)]
        allow_old_release: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// List the release branches on origin, newest first.
    Branches,
    /// Serve the release hub: the HTTP API (and optionally the built UI) over one clone of the repository.
    #[cfg(feature = "release-hub")]
    Serve {
        /// The clone (default: `[release_hub] repo_dir`, /opt/project).
        #[arg(long)]
        repo_dir: Option<std::path::PathBuf>,
        /// Address to listen on (default: `[release_hub] bind`, 0.0.0.0).
        #[arg(long)]
        bind: Option<String>,
        /// Port (default: `[release_hub] port`, 8765).
        #[arg(long)]
        port: Option<u16>,
        /// The built frontend to serve next to the API.
        #[arg(long)]
        ui_dir: Option<std::path::PathBuf>,
        /// Allow a frontend on this origin (e.g. http://localhost:5173 for `npm run dev`).
        #[arg(long)]
        cors_origin: Option<String>,
        /// Validate releases with dbt (`[release] validate`) in the clone before pushing.
        #[arg(long)]
        validate: bool,
    },
}

#[cfg(feature = "release")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PromoteKind {
    Auto,
    New,
    Patch,
}

#[cfg(feature = "release")]
#[derive(Debug, Subcommand)]
pub enum JiraCommand {
    /// Move tickets to their next status (`update_ticket.py`).
    Advance {
        /// Ticket keys, e.g. INTEL-9876 INTEL-8765.
        #[arg(long, num_args = 1.., required = true)]
        tickets: Vec<String>,
        /// Transition (or target status) name; default `[jira] advance_transition`, else the first one offered.
        #[arg(long)]
        transition: Option<String>,
    },
}

#[cfg(feature = "airflow")]
#[derive(Debug, Subcommand)]
pub enum AirflowCommand {
    /// Reset the project checkout on the Airflow host to a tag and link its DAGs.
    Sync {
        /// The git tag to deploy.
        #[arg(long)]
        tag: String,
    },
}

#[cfg(feature = "sql")]
#[derive(Debug, Subcommand)]
pub enum SqlCommand {
    /// Run SQL files and print their results.
    Run {
        /// SQL files; each may hold several statements (the last result set is shown).
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Also save each result as `<file stem>.csv` here.
        #[arg(long)]
        csv_dir: Option<PathBuf>,
        /// Rows to print per result.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Save a table as CSV.
    Export {
        /// database.table
        table: String,
        /// Output file (default: `data/<database>_<table>.csv`).
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[cfg(feature = "qa")]
#[derive(Debug, Subcommand)]
pub enum QaCommand {
    /// Table models that nothing (tests aside) reads.
    OrphanTables,
    /// Views whose downstream tables take long to build: candidates to materialize.
    ViewCandidates {
        /// CSV output (default: dbt_view_candidates.csv).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Do not read run timings from the database (all times are 0).
        #[arg(long)]
        no_timings: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    /// Show the environment, project, configuration sources and secret keys (never values).
    Show,
    /// Check the dbt engine, its SingleStore driver, the CA certificate, Vault and the dbt profile.
    Doctor,
}

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Derive dbt's command-line grammar from a dbt binary (for crates/vdp-dbt/grammar/dbt.json).
    Grammar {
        /// The dbt binary (default: the configured engine).
        #[arg(long)]
        dbt: Option<PathBuf>,
        /// Write here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

pub fn parse_environment(value: &str) -> Result<Environment, String> {
    value
        .parse()
        .map_err(|e: vdp_core::CoreError| e.to_string())
}

/// `vdp vaultspeed ...`. The environment is `--env`, else `$TARGET_BRANCH` (set by the framework's CI), else the
/// usual `$DBT_DEPLOY_TARGET`/default.
#[cfg(feature = "vaultspeed")]
#[derive(Debug, Args)]
pub struct VaultspeedArgs {
    /// The vaultspeed-automation-framework checkout (default: `[vaultspeed].project_dir`, the current directory).
    #[arg(long, global = true, env = "VDP_VAULTSPEED_DIR")]
    pub framework_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: VaultspeedCommand,
}

#[cfg(feature = "vaultspeed")]
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
}

#[cfg(feature = "vaultspeed")]
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

#[cfg(feature = "vaultspeed")]
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

#[cfg(feature = "vaultspeed")]
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum RuleArtifact {
    Ddl,
    Etl,
}

#[cfg(feature = "vaultspeed")]
#[derive(Debug, Subcommand)]
pub enum VaultspeedRulesCommand {
    /// Show how the pipeline splits, rewrites and classifies a SQL file.
    Test {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "ddl")]
        artifact: RuleArtifact,
    },
}

#[cfg(feature = "vaultspeed")]
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
