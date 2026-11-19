//! Command-line interface of `vdp`.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use vdp_core::Environment;

use crate::help;

/// The `vdp` command line: the [`GlobalArgs`] every command shares, and one [`Command`].
#[derive(Debug, Parser)]
#[command(
    name = "vdp",
    version,
    about = "VDP toolkit for dbt v2 on SingleStore: the dbt wrapper, source maintenance, releases and more",
    long_about = help::ROOT_LONG_ABOUT,
    after_help = help::ROOT_AFTER_HELP,
    after_long_help = help::ROOT_AFTER_LONG_HELP
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Command,
}

/// Options every command shares.
#[derive(Debug, Clone, Default, Args)]
pub struct GlobalArgs {
    /// Environment: LDV, ABT, CBT, PRD or a configured name
    #[arg(
        long = "env",
        global = true,
        help_heading = "Global options",
        value_name = "ENV",
        value_parser = parse_environment,
        long_help = help::GLOBAL_ENV_LONG
    )]
    pub environment: Option<Environment>,

    /// dbt project directory
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "VDP_PROJECT_DIR",
        value_name = "DIR",
        long_help = help::GLOBAL_PROJECT_DIR_LONG
    )]
    pub project_dir: Option<PathBuf>,

    /// Extra configuration file, applied last
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "VDP_CONFIG",
        value_name = "FILE",
        long_help = help::GLOBAL_CONFIG_LONG
    )]
    pub config: Option<PathBuf>,

    /// dbt target the toolkit itself uses
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        value_name = "TARGET",
        long_help = help::GLOBAL_TARGET_LONG
    )]
    pub target: Option<String>,

    /// Directory with profiles.yml
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        value_name = "DIR",
        long_help = help::GLOBAL_PROFILES_DIR_LONG
    )]
    pub profiles_dir: Option<PathBuf>,

    /// Do not log in to Vault
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "VDP_NO_VAULT",
        action = clap::ArgAction::SetTrue,
        value_parser = clap::builder::BoolishValueParser::new(),
        long_help = help::GLOBAL_NO_VAULT_LONG
    )]
    pub no_vault: bool,

    /// Debug logging
    #[arg(
        short,
        long,
        global = true,
        help_heading = "Global options",
        long_help = help::GLOBAL_VERBOSE_LONG
    )]
    pub verbose: bool,

    /// Warnings and errors only
    #[arg(
        short,
        long,
        global = true,
        help_heading = "Global options",
        long_help = help::GLOBAL_QUIET_LONG
    )]
    pub quiet: bool,

    /// Also write log events to FILE as NDJSON
    #[arg(
        long,
        global = true,
        help_heading = "Global options",
        env = "VDP_LOG_JSON",
        value_name = "FILE",
        long_help = help::GLOBAL_LOG_JSON_LONG
    )]
    pub log_json: Option<PathBuf>,
}

impl GlobalArgs {
    /// Default `tracing` filter level derived from `--verbose`, `--quiet`, and wrapper mode.
    #[must_use]
    pub const fn default_log_filter(&self, is_wrapper: bool) -> &'static str {
        if self.verbose {
            "debug"
        } else if self.quiet || is_wrapper {
            "warn"
        } else {
            "info"
        }
    }

    /// Default `tracing` filter level for the JSON file sink (`info` by default, `debug` with `-v`,
    /// unaffected by `-q`).
    #[must_use]
    pub const fn file_log_filter(&self) -> &'static str {
        if self.verbose { "debug" } else { "info" }
    }
}

impl Cli {
    /// Whether the command is `dbt`, the wrapper around the dbt engine.
    pub const fn is_wrapper(&self) -> bool {
        matches!(self.command, Command::Dbt { .. })
    }
}

/// The commands of `vdp`. Each module's commands are behind a feature of the same name.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run dbt through the wrapper (what the `dbt` link does): `vdp dbt build -s my_model`
    #[command(
        disable_help_flag = true,
        long_about = help::DBT_LONG_ABOUT,
        after_long_help = help::DBT_AFTER_LONG_HELP
    )]
    Dbt {
        /// Arguments for dbt, passed on unchanged
        #[arg(
            value_name = "DBT_ARGS",
            trailing_var_arg = true,
            allow_hyphen_values = true,
            num_args = 0..
        )]
        args: Vec<OsString>,
    },
    #[cfg(feature = "sources")]
    /// Maintain dbt sources from the SingleStore catalog
    #[command(subcommand, long_about = help::SOURCES_LONG_ABOUT)]
    Sources(SourcesCommand),
    #[cfg(feature = "release")]
    /// dbt DROP releases
    #[command(subcommand, long_about = help::RELEASE_LONG_ABOUT)]
    Release(ReleaseCommand),
    #[cfg(feature = "release")]
    /// Jira tickets
    #[command(subcommand, long_about = help::JIRA_LONG_ABOUT)]
    Jira(JiraCommand),
    #[cfg(feature = "sql")]
    /// Ad-hoc SQL against the dbt target's database
    #[command(subcommand, long_about = help::SQL_LONG_ABOUT)]
    Sql(SqlCommand),
    #[cfg(feature = "qa")]
    /// Quality checks on the dbt manifest
    #[command(subcommand, long_about = help::QA_LONG_ABOUT)]
    Qa(QaCommand),
    /// Inspect the resolved environment
    #[command(subcommand, long_about = help::ENV_LONG_ABOUT)]
    Env(EnvCommand),
    /// Maintenance of the toolkit itself
    #[command(subcommand, long_about = help::DEV_LONG_ABOUT)]
    Dev(DevCommand),
    /// A command of another binary of the toolkit (`vdp vaultspeed ...` runs `vdp-vaultspeed`, `vdp release-ui ...`
    /// runs `vdp-release-ui`). Holds the subcommand and everything after it; [`crate::external`] hands the original
    /// arguments over.
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

/// `vdp sources ...`: source maintenance against the SingleStore catalog.
#[cfg(feature = "sources")]
#[derive(Debug, Subcommand)]
pub enum SourcesCommand {
    /// Add tables to their sources, with their columns from the database
    #[command(
        long_about = help::SOURCES_ADD_LONG_ABOUT,
        after_long_help = help::SOURCES_ADD_AFTER_LONG_HELP
    )]
    Add {
        /// Tables as `database.table` (or `source.table` for a declared source)
        #[arg(value_name = "TABLE", long_help = help::SOURCES_ADD_TABLES_LONG)]
        tables: Vec<String>,
        /// Also add every source table dbt reports as undefined (runs `dbt parse`)
        #[arg(long, long_help = help::SOURCES_ADD_MISSING_LONG)]
        missing: bool,
        /// Show what would change without writing
        #[arg(long, long_help = help::SOURCES_ADD_DRY_RUN_LONG)]
        dry_run: bool,
    },
    /// Regenerate the generated source files from the database and keep only referenced tables
    #[command(
        long_about = help::SOURCES_REWRITE_LONG_ABOUT,
        after_long_help = help::SOURCES_REWRITE_AFTER_LONG_HELP
    )]
    Rewrite {
        /// Databases to regenerate (default: `[sources] databases`)
        #[arg(
            long,
            value_delimiter = ',',
            value_name = "DATABASE",
            long_help = help::SOURCES_REWRITE_DATABASES_LONG
        )]
        databases: Option<Vec<String>>,
        /// Regenerate only; do not run dbt or trim
        #[arg(long, long_help = help::SOURCES_REWRITE_NO_TRIM_LONG)]
        no_trim: bool,
        /// Show what would change without writing or deleting files
        #[arg(long, long_help = help::SOURCES_REWRITE_DRY_RUN_LONG)]
        dry_run: bool,
    },
}

/// `vdp release ...`: the DROP release flows. The release hub (`release serve`) is the separate `vdp-release-ui`
/// binary (see [`crate::external`]).
#[cfg(feature = "release")]
#[derive(Debug, Subcommand)]
pub enum ReleaseCommand {
    /// Cut a release branch: revert tickets outside the target DROP, validate with dbt, push
    #[command(
        long_about = help::RELEASE_CREATE_LONG_ABOUT,
        after_long_help = help::RELEASE_CREATE_AFTER_LONG_HELP
    )]
    Create {
        /// Release branch name, e.g. release/2026_DROP05
        #[arg(long, value_name = "BRANCH", long_help = help::RELEASE_CREATE_NAME_LONG)]
        name: String,
        /// Target Jira fix version, e.g. 2026_DROP05
        #[arg(long, value_name = "FIXVERSION", long_help = help::RELEASE_CREATE_FIXVERSION_LONG)]
        fixversion: String,
        /// Ref to cut the release from (default: `[release] from`)
        #[arg(long, value_name = "REF", long_help = help::RELEASE_CREATE_FROM_LONG)]
        from: Option<String>,
        /// Base ref for the merge-base (default: `[release] to`)
        #[arg(long, value_name = "REF", long_help = help::RELEASE_CREATE_TO_LONG)]
        to: Option<String>,
        /// Keep tickets without a DROP fix version
        #[arg(long, long_help = help::RELEASE_CREATE_KEEP_EMPTY_LONG)]
        keep_empty_fixversions: bool,
        /// Ask before each revert or cherry-pick
        #[arg(long, long_help = help::RELEASE_CREATE_INTERACTIVE_LONG)]
        interactive: bool,
        /// Recreate the branch if it exists, and force-push (with lease)
        #[arg(long, long_help = help::RELEASE_CREATE_FORCE_LONG)]
        force: bool,
        /// Accept dropping commits only the old branch has
        #[arg(long, long_help = help::RELEASE_CREATE_ACCEPT_LOSSES_LONG)]
        accept_losses: bool,
        /// Do not commit `changelog/<fixversion>.md`
        #[arg(long, long_help = help::RELEASE_CREATE_NO_CHANGELOG_LONG)]
        no_changelog: bool,
        /// Allow a release branch outside the newest two
        #[arg(long, long_help = help::RELEASE_CREATE_ALLOW_OLD_LONG)]
        allow_old_release: bool,
        /// Analyse and print the plan; change nothing
        #[arg(long, long_help = help::RELEASE_CREATE_DRY_RUN_LONG)]
        dry_run: bool,
    },
    /// Show what a release (or with `--patch <branch>`, a patch) would contain
    #[command(
        long_about = help::RELEASE_ANALYSE_LONG_ABOUT,
        after_long_help = help::RELEASE_ANALYSE_AFTER_LONG_HELP
    )]
    Analyse {
        /// Target Jira fix version, e.g. 2026_DROP05
        #[arg(long, value_name = "FIXVERSION", long_help = help::RELEASE_ANALYSE_FIXVERSION_LONG)]
        fixversion: Option<String>,
        /// Source ref (default: `[release] from`)
        #[arg(long, value_name = "REF", long_help = help::RELEASE_ANALYSE_FROM_LONG)]
        from: Option<String>,
        /// Compare ref (default: `[release] to`)
        #[arg(long, value_name = "REF", long_help = help::RELEASE_ANALYSE_TO_LONG)]
        to: Option<String>,
        /// Analyse a patch of this release branch instead
        #[arg(long, value_name = "BRANCH", long_help = help::RELEASE_ANALYSE_PATCH_LONG)]
        patch: Option<String>,
        /// Print the analysis as JSON
        #[arg(long, long_help = help::RELEASE_ANALYSE_JSON_LONG)]
        json: bool,
    },
    /// Add tickets to a release branch (re-enable reverted ones, cherry-pick new ones) and push
    #[command(
        long_about = help::RELEASE_PATCH_LONG_ABOUT,
        after_long_help = help::RELEASE_PATCH_AFTER_LONG_HELP
    )]
    Patch {
        /// The release branch, e.g. release/2026_DROP24
        #[arg(value_name = "BRANCH", long_help = help::RELEASE_PATCH_BRANCH_LONG)]
        branch: String,
        /// Tickets to add (default: emergency drops and re-enables)
        #[arg(
            long,
            value_delimiter = ',',
            value_name = "TICKET",
            long_help = help::RELEASE_PATCH_INCLUDE_LONG
        )]
        include: Option<Vec<String>>,
        /// Target fix version (default: the branch suffix)
        #[arg(long, value_name = "FIXVERSION", long_help = help::RELEASE_PATCH_FIXVERSION_LONG)]
        fixversion: Option<String>,
        /// Source ref (default: `[release] from`)
        #[arg(long, value_name = "REF", long_help = help::RELEASE_PATCH_FROM_LONG)]
        from: Option<String>,
        /// Ask before each re-enable or cherry-pick
        #[arg(long, long_help = help::RELEASE_PATCH_INTERACTIVE_LONG)]
        interactive: bool,
        /// Allow patching a release branch outside the newest two
        #[arg(long, long_help = help::RELEASE_PATCH_ALLOW_OLD_LONG)]
        allow_old_release: bool,
        /// Do not rewrite the release changelog
        #[arg(long, long_help = help::RELEASE_PATCH_NO_CHANGELOG_LONG)]
        no_changelog: bool,
        /// Analyse and print the plan; change nothing
        #[arg(long, long_help = help::RELEASE_PATCH_DRY_RUN_LONG)]
        dry_run: bool,
    },
    /// Bring a release branch to main, as a new release or a patch, and push main with a lease
    #[command(
        long_about = help::RELEASE_PROMOTE_LONG_ABOUT,
        after_long_help = help::RELEASE_PROMOTE_AFTER_LONG_HELP
    )]
    Promote {
        /// The release branch
        #[arg(value_name = "BRANCH", long_help = help::RELEASE_PROMOTE_BRANCH_LONG)]
        branch: String,
        /// Kind of promotion
        #[arg(
            long,
            value_enum,
            default_value_t = PromoteKind::Auto,
            long_help = help::RELEASE_PROMOTE_KIND_LONG
        )]
        kind: PromoteKind,
        /// Main has a newer release: promote this older one anyway (a rollback)
        #[arg(long, long_help = help::RELEASE_PROMOTE_ACCEPT_ROLLBACK_LONG)]
        accept_rollback: bool,
        /// Accept dropping commits only main has
        #[arg(long, long_help = help::RELEASE_PROMOTE_ACCEPT_LOSSES_LONG)]
        accept_losses: bool,
        /// Allow promoting a release branch outside the newest two
        #[arg(long, long_help = help::RELEASE_PROMOTE_ALLOW_OLD_LONG)]
        allow_old_release: bool,
        /// Print what would happen; change nothing
        #[arg(long, long_help = help::RELEASE_PROMOTE_DRY_RUN_LONG)]
        dry_run: bool,
    },
    /// List the release branches on origin, newest first
    #[command(
        long_about = help::RELEASE_BRANCHES_LONG_ABOUT,
        after_long_help = help::RELEASE_BRANCHES_AFTER_LONG_HELP
    )]
    Branches,
}

/// Which kind of promotion to run: detected (`auto`), a new release or a patch of the release main has.
#[cfg(feature = "release")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PromoteKind {
    /// Decide by ancestry: a patch when main has part of the branch, else new
    Auto,
    /// Reset main to the merge-base, then merge the release
    New,
    /// Merge a patch onto the release main already has
    Patch,
}

/// `vdp jira ...`: Jira tickets.
#[cfg(feature = "release")]
#[derive(Debug, Subcommand)]
pub enum JiraCommand {
    /// Move tickets to their next status (`update_ticket.py`)
    #[command(
        long_about = help::JIRA_ADVANCE_LONG_ABOUT,
        after_long_help = help::JIRA_ADVANCE_AFTER_LONG_HELP
    )]
    Advance {
        /// Ticket keys, e.g. INTEL-9876 INTEL-8765
        #[arg(
            long,
            num_args = 1..,
            required = true,
            value_name = "TICKET",
            long_help = help::JIRA_ADVANCE_TICKETS_LONG
        )]
        tickets: Vec<String>,
        /// Transition (or target status) name
        #[arg(long, value_name = "NAME", long_help = help::JIRA_ADVANCE_TRANSITION_LONG)]
        transition: Option<String>,
        /// Print what would change; modify no tickets
        #[arg(long, long_help = help::JIRA_ADVANCE_DRY_RUN_LONG)]
        dry_run: bool,
    },
}

/// `vdp sql ...`: SQL against SingleStore.
#[cfg(feature = "sql")]
#[derive(Debug, Subcommand)]
pub enum SqlCommand {
    /// Run SQL files and print their results
    #[command(
        long_about = help::SQL_RUN_LONG_ABOUT,
        after_long_help = help::SQL_RUN_AFTER_LONG_HELP
    )]
    Run {
        /// SQL files; each may hold several statements
        #[arg(required = true, value_name = "FILE", long_help = help::SQL_RUN_FILES_LONG)]
        files: Vec<PathBuf>,
        /// Also save each result as `<file stem>.csv` here
        #[arg(long, value_name = "DIR", long_help = help::SQL_RUN_CSV_DIR_LONG)]
        csv_dir: Option<PathBuf>,
        /// Rows to print per result
        #[arg(
            long,
            default_value_t = 50,
            value_name = "ROWS",
            long_help = help::SQL_RUN_LIMIT_LONG
        )]
        limit: usize,
        /// Print what would run; do not connect or write CSV files
        #[arg(long, long_help = help::SQL_RUN_DRY_RUN_LONG)]
        dry_run: bool,
    },
    /// Save a table as CSV
    #[command(
        long_about = help::SQL_EXPORT_LONG_ABOUT,
        after_long_help = help::SQL_EXPORT_AFTER_LONG_HELP
    )]
    Export {
        /// The table as database.table
        #[arg(value_name = "TABLE", long_help = help::SQL_EXPORT_TABLE_LONG)]
        table: String,
        /// Output file (default: `data/<database>_<table>.csv`)
        #[arg(long, value_name = "FILE", long_help = help::SQL_EXPORT_OUT_LONG)]
        out: Option<PathBuf>,
    },
}

/// `vdp qa ...`: analyses of the dbt manifest.
#[cfg(feature = "qa")]
#[derive(Debug, Subcommand)]
pub enum QaCommand {
    /// Table models that nothing (tests aside) reads
    #[command(
        long_about = help::QA_ORPHAN_TABLES_LONG_ABOUT,
        after_long_help = help::QA_ORPHAN_TABLES_AFTER_LONG_HELP
    )]
    OrphanTables,
    /// Views whose downstream tables take long to build: candidates to materialize
    #[command(
        long_about = help::QA_VIEW_CANDIDATES_LONG_ABOUT,
        after_long_help = help::QA_VIEW_CANDIDATES_AFTER_LONG_HELP
    )]
    ViewCandidates {
        /// CSV output (default: dbt_view_candidates.csv)
        #[arg(long, value_name = "FILE", long_help = help::QA_VIEW_CANDIDATES_OUT_LONG)]
        out: Option<PathBuf>,
        /// Do not read run timings from the database (all times are 0)
        #[arg(long, long_help = help::QA_VIEW_CANDIDATES_NO_TIMINGS_LONG)]
        no_timings: bool,
    },
}

/// `vdp env ...`: what the toolkit sees of the environment.
#[derive(Debug, Subcommand)]
pub enum EnvCommand {
    /// Show the environment, project, configuration sources and secret keys (never values)
    #[command(
        long_about = help::ENV_SHOW_LONG_ABOUT,
        after_long_help = help::ENV_SHOW_AFTER_LONG_HELP
    )]
    Show,
    /// Check the dbt engine, the SingleStore driver, the CA certificate, Vault and the profile
    #[command(
        long_about = help::ENV_DOCTOR_LONG_ABOUT,
        after_long_help = help::ENV_DOCTOR_AFTER_LONG_HELP
    )]
    Doctor,
}

/// `vdp dev ...`: commands for developing the toolkit itself.
#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// Derive dbt's command-line grammar from a dbt binary (for crates/vdp-dbt/grammar/dbt.json)
    #[command(
        long_about = help::DEV_GRAMMAR_LONG_ABOUT,
        after_long_help = help::DEV_GRAMMAR_AFTER_LONG_HELP
    )]
    Grammar {
        /// The dbt binary (default: the configured engine)
        #[arg(long, value_name = "PATH", long_help = help::DEV_GRAMMAR_DBT_LONG)]
        dbt: Option<PathBuf>,
        /// Write here instead of stdout
        #[arg(long, value_name = "FILE", long_help = help::DEV_GRAMMAR_OUT_LONG)]
        out: Option<PathBuf>,
    },
}

/// Reads an environment name for clap.
///
/// # Errors
///
/// The message of the [`vdp_core::CoreError`] when `value` is not a valid environment name (empty, or a character other
/// than letters, digits, `_` and `-`).
pub fn parse_environment(value: &str) -> Result<Environment, String> {
    value
        .parse()
        .map_err(|e: vdp_core::CoreError| vdp_core::display_chain(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_existing_and_new_global_flags_before_and_after_subcommand() {
        let before = Cli::try_parse_from([
            "vdp",
            "--env",
            "LDV",
            "--project-dir",
            "/tmp/proj",
            "--config",
            "/tmp/vdp.toml",
            "--target",
            "dev",
            "--profiles-dir",
            "/tmp/prof",
            "--no-vault",
            "-v",
            "--log-json",
            "/tmp/run.json",
            "env",
            "show",
        ])
        .unwrap();
        assert_eq!(before.global.environment, Some(Environment::LDV));
        assert_eq!(before.global.project_dir, Some(PathBuf::from("/tmp/proj")));
        assert_eq!(before.global.config, Some(PathBuf::from("/tmp/vdp.toml")));
        assert_eq!(before.global.target.as_deref(), Some("dev"));
        assert_eq!(before.global.profiles_dir, Some(PathBuf::from("/tmp/prof")));
        assert!(before.global.no_vault);
        assert!(before.global.verbose);
        assert!(!before.global.quiet);
        assert_eq!(before.global.log_json, Some(PathBuf::from("/tmp/run.json")));

        let after = Cli::try_parse_from([
            "vdp",
            "env",
            "show",
            "-q",
            "--log-json",
            "/tmp/quiet.json",
            "--env",
            "PRD",
        ])
        .unwrap();
        assert_eq!(after.global.environment, Some(Environment::PRD));
        assert!(!after.global.verbose);
        assert!(after.global.quiet);
        assert_eq!(
            after.global.log_json,
            Some(PathBuf::from("/tmp/quiet.json"))
        );
    }

    #[test]
    fn default_and_file_log_filters_honour_verbose_quiet_and_wrapper() {
        let normal = GlobalArgs::default();
        assert_eq!(normal.default_log_filter(false), "info");
        assert_eq!(normal.default_log_filter(true), "warn");
        assert_eq!(normal.file_log_filter(), "info");

        let quiet = GlobalArgs {
            quiet: true,
            ..GlobalArgs::default()
        };
        assert_eq!(quiet.default_log_filter(false), "warn");
        assert_eq!(quiet.file_log_filter(), "info");

        let verbose = GlobalArgs {
            verbose: true,
            quiet: true,
            ..GlobalArgs::default()
        };
        assert_eq!(verbose.default_log_filter(true), "debug");
        assert_eq!(verbose.file_log_filter(), "debug");
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
        walk(&root, "vdp", &mut |path, command| {
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
        walk(&root, "vdp", &mut |path, command| {
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
