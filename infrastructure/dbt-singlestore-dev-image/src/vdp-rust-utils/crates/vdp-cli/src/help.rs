//! The long help (`--help`) of `vdp`: what each command reads, what it changes, and examples.
//!
//! The short help (`-h`) is the one-line `about` and `help` in [`crate::args`]. The texts here are wrapped by hand
//! (clap does not wrap without its `wrap_help` feature): lines of a `long_about` or `after_long_help` stay within
//! 96 columns, lines of an option's `long_help` within 86 (clap indents those by 8).

// ---- the root command -------------------------------------------------------------------------------------------

pub(crate) const ROOT_LONG_ABOUT: &str = "\
VDP toolkit for dbt v2 on SingleStore.

One binary for the work around a dbt project: the dbt wrapper (also installed as `dbt`), source
maintenance, DROP releases and Jira, ad-hoc SQL and quality checks on the dbt manifest. Each command
finds what it needs (the dbt project, the database, Vault, Jira, git) through the configuration and
secrets described below. `-h` gives a summary; `vdp <command> --help` has the details and examples.";

pub(crate) const ROOT_AFTER_HELP: &str = "\
VaultSpeed deployments (`vdp vaultspeed ...`) are run by the separate `vdp-vaultspeed` binary,
and the release hub (`vdp release-ui release serve`) by `vdp-release-ui`.";

pub(crate) const ROOT_AFTER_LONG_HELP: &str = "\
Command groups:
  dbt                     run dbt v2 with the organisation's environment around it
  sources                 add tables to dbt sources; regenerate the generated source files
  release, jira           cut, patch and promote DROP release branches; move Jira tickets on
  sql, qa                 run SQL files and export tables; analyse the dbt manifest
  env, dev                inspect the resolved environment; maintain the toolkit itself
  vaultspeed, release-ui  hand-offs to other binaries (see \"Other binaries\")

Environments (--env):
  LDV, ABT, CBT, PRD, or a name configured as [environments.<name>]. `dev`, `val` and `prd` are
  aliases of LDV, ABT and PRD (CBT has none). Without --env the environment is $DBT_DEPLOY_TARGET,
  else [core].default_environment (LDV unless configured). An unknown name stops the command with
  exit code 2. `vdp env show` prints what was selected.

Configuration:
  Settings are layered, later files winning key by key:
    built-in defaults < /etc/vdp/vdp.toml < <dbt project>/vdp.toml < --config FILE ($VDP_CONFIG)
  The dbt project is --project-dir ($VDP_PROJECT_DIR), else the nearest parent directory of the
  current one that has a dbt_project.yml. Secrets come from Vault, the project's dotenv file and the
  process environment, in the order of [secrets].precedence; --no-vault skips Vault. The help of
  each command names the sections it reads ([sources], [release], [jira], [qa], [dbt], ...).

Exit codes:
  0  success
  1  the command ran, but some of its work failed: a batch with failures, a failed check
  2  setup or usage error: bad arguments, unreadable configuration, unknown environment, a service
     that cannot be reached, a refused operation
  Commands whose exit codes differ from this say so. `vdp dbt` returns dbt's own exit code.

Logging:
  Logs go to stderr: info by default, debug with -v, warnings and errors only with -q. $VDP_LOG (a
  tracing filter such as `debug`) overrides the level. --log-json FILE also writes the events as
  one JSON object per line: info and above (debug with -v), whatever -q says.

Other binaries:
  vdp vaultspeed ...   runs `vdp-vaultspeed`: VaultSpeed deployments (plan, deploy, ledger, rules,
                       dags, history), in the vaultspeed-framework workspace
  vdp release-ui ...   runs `vdp-release-ui`: the release hub behind the release UI
                       (`vdp release-ui release serve`), in the release-ui workspace
  `vdp` replaces itself with that binary, looked up next to `vdp` and then on PATH, and passes the
  arguments on in order, global options before the command included. Its exit code is the result.
  When the binary is not found, `vdp` says so and exits with code 2.

Examples:
  # Show the environment, project, configuration files and secret keys (never values)
  vdp env show

  # Check the dbt engine, the SingleStore driver, the CA certificate, Vault and the dbt profile
  vdp --env dev env doctor

  # Add every source table that dbt reports as undefined
  vdp --env dev sources add --missing

  # See what a release would contain, then run dbt as the wrapper does
  vdp --env prd release analyse --fixversion 2026_DROP05
  vdp dbt build -s my_model";

// ---- dbt --------------------------------------------------------------------------------------------------------

pub(crate) const DBT_LONG_ABOUT: &str = "\
Run dbt v2 through the wrapper: what the `dbt` link to `vdp` does, without needing the link.

Everything from the first argument that is not a `vdp` global option belongs to dbt and reaches
it unchanged: `vdp --env dev dbt build --target x` gives dbt `build --target x`, and
`vdp dbt --help` is dbt's own help (`vdp help dbt` shows this text). Around the engine the
wrapper:

  - finds the dbt v2 binary: [dbt].binary, else $VDP_DBT_BIN, else the first `dbt` on PATH that is
    not this wrapper;
  - uses the project of dbt's own `--project-dir` when given (so its vdp.toml, dotenv file and
    secrets match what dbt reads), else the one of `vdp`'s global options;
  - hands dbt the secrets (dotenv file, Vault), DBT_ALLOW_EXPERIMENTAL_ADAPTERS, VDP_ENV (the
    environment, `LDV` for `dev`) and VDP_SSL_CA (the environment's CA certificate);
  - refuses `build` and `run` without `-s`, `--select` or `--selector`, unless the SingleStore user
    is a service user (a name ending in `_dbt` and not starting with `pu`);
  - after a failed run with undefined sources, prints `hint: vdp sources add <source>.<table>`.

Reads [dbt], [core], [secrets] and the environment's settings. It changes whatever the dbt command
changes. By default only warnings and errors are logged: the output is dbt's.

Exit codes: dbt's own (a death by signal is 128+n); 1 when the guard refuses; 2 when dbt cannot be
started or the setup fails.";

pub(crate) const DBT_AFTER_LONG_HELP: &str = "\
Examples:
  # Build one model
  vdp dbt build -s my_model

  # Run a selection against the dev environment
  vdp --env dev dbt run --select tag:daily+

  # Compile a model and its parents
  vdp dbt compile -s +my_model

  # Run a macro with arguments
  vdp dbt run-operation my_macro --args '{days: 7}'";

// ---- sources ----------------------------------------------------------------------------------------------------

#[cfg(feature = "sources")]
pub(crate) const SOURCES_LONG_ABOUT: &str = "\
Maintain dbt sources from the SingleStore catalog.

`add` puts tables, with their columns, into source files; `rewrite` regenerates the generated source
files from the database. Both read [sources] (generated_dir, databases, include_columns,
column_order, database_prefix), look tables up in information_schema over the connection of the dbt
profile's target (--target, --profiles-dir), and write YAML files below the dbt project. Neither
changes the database.

Generated source files are models/sources/sources_<database>.yml ([sources].generated_dir). A source
declared in a hand-written YAML file is edited in place by `add` and left alone by `rewrite`.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_ADD_LONG_ABOUT: &str = "\
Add tables to their sources, with their columns from the database.

Give the tables as `database.table` (or `source.table` for a source declared by hand), and/or use
--missing to add every table that dbt reports as an undefined source (dbt1048).

A source declared in a hand-written YAML file gets the table inserted there, leaving the rest of the
file untouched. Any other source lives in models/sources/sources_<database>.yml, which keeps the
descriptions, tests and meta people added; a table it already has gets its columns refreshed. A
table the database does not have is reported with close matches, and nothing is written for it.

Reads: [sources]; the database (information_schema); with --missing, `dbt parse`.
Changes: source YAML files only. After adding with --missing, dbt parses again and the command
fails if sources are still undefined.
--dry-run prints `would add ...` lines and writes nothing (--missing still runs the first parse).

Exit codes: 1 when a table could not be added (not in the database, or its file cannot be edited)
or dbt still reports undefined sources; 2 when there is nothing to add (no tables and no --missing)
or `dbt parse` fails for another reason.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_ADD_AFTER_LONG_HELP: &str = "\
Examples:
  # Add one table, with its columns, to its source file
  vdp --env dev sources add dev_landing.orders

  # Add several tables; preview first
  vdp --env dev sources add dev_landing.orders dev_landing.customers --dry-run

  # Add every source dbt reports as undefined, then confirm with another dbt parse
  vdp --env dev sources add --missing";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_ADD_TABLES_LONG: &str = "\
Tables to add: `database.table`, or `source.table` for a source declared in a
hand-written YAML file. Where [sources.database_prefix] is configured, use the
physical database name (`dev_landing.orders`); the file then says the
environment-independent expression instead.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_ADD_MISSING_LONG: &str = "\
Run `dbt parse`, collect the tables it reports as undefined sources (dbt1048) and add
them to the tables given. Afterwards dbt parses again to confirm none is left (not
with --dry-run).";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_ADD_DRY_RUN_LONG: &str = "\
Show what would change without writing: one `would add ...` line per table, naming its
file.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_REWRITE_LONG_ABOUT: &str = "\
Regenerate the generated source files from the database and keep only referenced tables.

Step 1 writes every table of each database to its generated file (sources_<database>.yml under
[sources].generated_dir), keeping the descriptions, tests and meta people added. A database whose
source is declared in a hand-written file is skipped. Step 2 runs `dbt parse` and trims the
generated files to the tables the project references; a file left with none is deleted.

Reads: [sources] (databases, include_columns, column_order, database_prefix); the database
(information_schema); the dbt manifest from `dbt parse`.
Changes: the generated source files (written, trimmed or deleted). Not the database.
--dry-run lists what would be written per database and skips `dbt parse` and the trim.

Exit codes: 2 when `dbt parse` fails (the regenerated files are kept untrimmed).";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_REWRITE_AFTER_LONG_HELP: &str = "\
Examples:
  # Regenerate the databases of [sources].databases and trim to what the project uses
  vdp --env dev sources rewrite

  # See what would be written, for two databases only
  vdp --env dev sources rewrite --databases dev_landing,dev_edw_dv_rdv --dry-run

  # Regenerate without running dbt or trimming
  vdp --env dev sources rewrite --no-trim";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_REWRITE_DATABASES_LONG: &str = "\
Databases to regenerate, separated by commas (physical names, e.g. `dev_landing`).
Default: the list in [sources].databases. Each one gets its own generated file.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_REWRITE_NO_TRIM_LONG: &str = "\
Regenerate only: no `dbt parse`, so unreferenced tables stay in the regenerated files.
Without it, the files are trimmed to the tables the project references.";

#[cfg(feature = "sources")]
pub(crate) const SOURCES_REWRITE_DRY_RUN_LONG: &str = "\
Print, per database, how many tables would go to which file and whether the file would
be created, changed or unchanged. Nothing is written or deleted, and `dbt parse` and
the trim are skipped.";

// ---- release ----------------------------------------------------------------------------------------------------

#[cfg(feature = "release")]
pub(crate) const RELEASE_LONG_ABOUT: &str = "\
dbt DROP releases: analyse, cut, patch and promote release branches, and list them.

Releases follow the modified GitFlow: `dev` is cut into a release branch (`release/2026_DROP05`)
from which the tickets of later DROPs are reverted, validated with dbt, and pushed; patches add
tickets to a release branch; promoting brings a release branch to `main`. All commands work on the
git repository of the dbt project, fetch `origin` first (a failed fetch is a warning), and read
[release] (ticket_prefix, from, to, branch_prefix, validate) and [jira] (base_url, secrets,
excluded_fix_versions). Jira credentials are the secrets JIRA_USER and JIRA_TOKEN (names set in
[jira]); they are never prompted for.

Only release branches are written (`main` only by `promote`), never `dev`, and a command that
writes refuses a work tree with uncommitted changes. `create`, `patch` and `promote` have a
--dry-run that analyses and prints the plan and changes nothing. Unless --allow-old-release is
given, only the newest two release branches can change.

Serving these flows to the release UI is `release serve`, a command of the `vdp-release-ui` binary
(`vdp release-ui release serve`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_LONG_ABOUT: &str = "\
Cut a release branch: revert tickets outside the target DROP, validate with dbt, push.

The release branch is cut from --from (default [release] from, origin/dev). Each ticket merged
since the merge-base with --to is classified from Jira against --fixversion: tickets of an
emergency drop (EDROP) are always kept, tickets whose latest DROP is after the target are
reverted, tickets without a DROP fix version are reverted unless --keep-empty-fixversions, and a
fix version listed in [jira].excluded_fix_versions reverts the ticket. A ticket Jira cannot be
asked about stops the release (the error names it).

Steps: revert each ticket that is out (a revert already made on --to is cherry-picked instead),
commit `changelog/<fixversion>.md` as the last commit, run [release].validate (default
`compile --no-partial-parse`) in the project, and only then push the branch to origin.
An existing branch on origin is refused unless --force, which recreates it with a lease on the head
the analysis saw; commits only the old branch has are listed and need --accept-losses.

Reads: [release], [jira], Jira, git, dbt.
Changes: a local release branch and its push to origin. A work tree with uncommitted changes is
refused.
--dry-run prints the analysis and the planned steps and changes nothing (Jira and origin are still
read).
--interactive asks `Proceed? [Y/n]` on stdin before each step; `n` skips that step.

Exit codes: a refusal or failure (existing branch, dirty work tree, failed validation, rejected
push, Jira error, a conflict you abort) is an error: exit code 2.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_AFTER_LONG_HELP: &str = "\
Examples:
  # Preview the release: the analysis and the steps, nothing changed
  vdp --env prd release create --name release/2026_DROP05 --fixversion 2026_DROP05 --dry-run

  # Cut it
  vdp --env prd release create --name release/2026_DROP05 --fixversion 2026_DROP05

  # Recreate a release branch that exists, after reading what the recreation drops
  vdp --env prd release create --name release/2026_DROP05 --fixversion 2026_DROP05 --force --dry-run
  vdp --env prd release create --name release/2026_DROP05 --fixversion 2026_DROP05 --force \\
    --accept-losses

  # Confirm each revert one by one
  vdp --env prd release create --name release/2026_DROP05 --fixversion 2026_DROP05 --interactive";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_NAME_LONG: &str = "\
The release branch to create, e.g. `release/2026_DROP05`. It must start with
[release].branch_prefix (`release/`); `dev` and `main` are refused. A new release must
be newer than the newest one.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_FIXVERSION_LONG: &str = "\
The Jira fix version this release delivers, e.g. `2026_DROP05` (an emergency drop is
`2026_EDROP01`). Tickets whose latest DROP fix version is after it are reverted.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_FROM_LONG: &str = "\
The ref the release branch is cut from. Default: [release].from (`origin/dev`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_TO_LONG: &str = "\
The base ref: the merge-base with --from bounds the tickets considered, and reverts
already on it are carried over. Default: [release].to (`origin/main`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_KEEP_EMPTY_LONG: &str = "\
Keep tickets that have no DROP fix version in Jira in the release. By default they are
reverted. A ticket whose fix version is in [jira].excluded_fix_versions is reverted
either way.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_INTERACTIVE_LONG: &str = "\
Before each revert or cherry-pick, print the step and ask `Proceed? [Y/n]` on stdin.
Enter or `y` applies it; anything else skips that step and the branch keeps the ticket
as it is.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_FORCE_LONG: &str = "\
Recreate the branch when it exists: the local branch is replaced and the push replaces
the one on origin, with a lease on the head the analysis saw (a push made since is not
overwritten). Without it a branch that exists on origin is refused. Commits only the
old branch has are dropped: see --accept-losses.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_ACCEPT_LOSSES_LONG: &str = "\
Recreating the branch drops the commits only the old branch has (listed by --dry-run).
Without this flag they stop the release; with it they are dropped.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_NO_CHANGELOG_LONG: &str = "\
Do not commit `changelog/<fixversion>.md` (the tickets accepted, added and excluded)
as the last commit of the branch.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_ALLOW_OLD_LONG: &str = "\
Skip the scope check: by default a new release must be newer than the newest release
branch, and only one of the newest two can be recreated (never one already promoted to
main).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_CREATE_DRY_RUN_LONG: &str = "\
Analyse and print the plan, change nothing: no branch, no commit, no push. Jira and
origin are still read. With --force it also lists the commits the recreation would
drop.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_LONG_ABOUT: &str = "\
Show what a release (or with `--patch <branch>`, a patch) would contain.

For a release it lists the tickets merged on --from since the merge-base with --to, each marked
`revert` (out of the release) or `keep`, with the reason from Jira and the policy. For a patch of
an existing release branch it lists the tickets that could be added (marked `include`: emergency
drops and tickets whose revert on the branch can be undone), and those already on it.

Reads: [release], [jira], Jira, git. Fetches origin. Changes nothing else. Jira is optional: a
ticket Jira cannot be asked about shows its error instead of a decision.
--json prints the analysis as JSON on stdout, for other tools, and leaves out the progress messages.

Exit codes: 2 when --fixversion is missing for a release, or a ref or branch cannot be found.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_AFTER_LONG_HELP: &str = "\
Examples:
  # What would release 2026_DROP05 contain?
  vdp --env prd release analyse --fixversion 2026_DROP05

  # What could still be added to the existing release branch 2026_DROP04?
  vdp --env prd release analyse --patch release/2026_DROP04

  # The same analysis as JSON, for a script
  vdp --env prd release analyse --fixversion 2026_DROP05 --json";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_FIXVERSION_LONG: &str = "\
The Jira fix version to analyse for, e.g. `2026_DROP05`. Required for a release. With
--patch it defaults to the suffix of the branch name (`release/2026_DROP04` gives
`2026_DROP04`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_FROM_LONG: &str = "\
The source ref whose tickets are analysed. Default: [release].from (`origin/dev`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_TO_LONG: &str = "\
The ref compared against: the merge-base with --from bounds the tickets. Default:
[release].to (`origin/main`). Not used with --patch, which compares with the release
branch on origin.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_PATCH_LONG: &str = "\
Analyse a patch of this release branch (e.g. `release/2026_DROP04`) instead of a new
release: the branch on origin is the compare ref and --to is ignored.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_ANALYSE_JSON_LONG: &str = "\
Print the analysis as pretty JSON on stdout instead of the table. The progress
messages are not logged in this mode.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_LONG_ABOUT: &str = "\
Add tickets to an existing release branch (re-enable reverted ones, cherry-pick new ones) and push.

The tickets to add are --include, else the analysis' defaults (what `release analyse --patch` marks
`include`: emergency drops and tickets that can be re-enabled). A ticket reverted on the branch is
re-enabled by reverting the revert; a new one is cherry-picked as its whole mainline merge. The
changelog `changelog/<fixversion>.md` is rewritten as the branch's last commit, and the branch is
pushed without force, with a lease on the head the analysis saw.

Reads: [release], [jira], Jira, git. Fetches origin.
Changes: the local release branch (reset to origin's) and its push to origin. A work tree with
uncommitted changes is refused. Only the newest two release branches can be patched, unless
--allow-old-release.
--dry-run prints the analysis and the planned steps and changes nothing.

Exit codes: a refusal or failure (dirty work tree, a ticket not in the analysis, push rejected,
aborted) is an error: exit code 2.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_AFTER_LONG_HELP: &str = "\
Examples:
  # Preview the patch with the default selection
  vdp --env prd release patch release/2026_DROP05 --dry-run

  # Add two named tickets
  vdp --env prd release patch release/2026_DROP05 --include INTEL-1234,INTEL-5678

  # Confirm each step, without rewriting the changelog
  vdp --env prd release patch release/2026_DROP05 --interactive --no-changelog";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_BRANCH_LONG: &str = "\
The release branch to patch, e.g. `release/2026_DROP05`. It must exist on origin and
be one of the newest two release branches (see --allow-old-release).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_INCLUDE_LONG: &str = "\
The tickets to add, separated by commas (e.g. `INTEL-1234,INTEL-5678`; case does not
matter). They must be tickets of the patch analysis. Default: the tickets `release
analyse --patch` marks `include`: emergency drops and re-enable candidates.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_FIXVERSION_LONG: &str = "\
The fix version of the patch. Default: the suffix of the branch name
(`release/2026_DROP05` gives `2026_DROP05`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_FROM_LONG: &str = "\
The source ref the tickets are taken from. Default: [release].from (`origin/dev`).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_INTERACTIVE_LONG: &str = "\
Before each re-enable or cherry-pick, print the step and ask `Proceed? [Y/n]` on
stdin. Enter or `y` applies it; anything else skips that step.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_ALLOW_OLD_LONG: &str = "\
Skip the scope check, which allows patching only one of the newest two release
branches.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_NO_CHANGELOG_LONG: &str = "\
Leave `changelog/<fixversion>.md` as it is instead of rewriting it as the branch's
last commit.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PATCH_DRY_RUN_LONG: &str = "\
Analyse and print the plan, change nothing: nothing is checked out, committed or
pushed.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_LONG_ABOUT: &str = "\
Bring a release branch to main: a new release resets main to the merge-base and merges; a patch
merges onto main. Pushed with a lease on main as it was.

The kind is detected from ancestry (--kind auto): when the release is already on main there is
nothing to promote; when main has part of the release that `dev` lacks (its reverts, its changelog)
it is a patch, merged with `--no-ff` on top of main; otherwise it is a new release: main is reset
to the merge-base of `dev` and main, the release is merged with `--no-ff`, and main is pushed. The
reset drops commits that only main has: they are listed, and need --accept-losses. Promoting a
release older than the newest one main has is a rollback and needs --accept-rollback.

Reads: [release], git. Fetches origin; no Jira.
Changes: the local main (reset and merge) and its push to origin, leased on the head main had when
the command started. A work tree with uncommitted changes is refused. Only the newest two release
branches can be promoted, unless --allow-old-release.
--dry-run prints the detected kind, the commits a reset would drop and the planned steps, and
changes nothing.

Exit codes: a refusal or failure (not accepted rollback or losses, dirty work tree, push rejected,
aborted) is an error: exit code 2.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_AFTER_LONG_HELP: &str = "\
Examples:
  # See what promoting would do: the kind, what a reset drops
  vdp --env prd release promote release/2026_DROP05 --dry-run

  # Promote, with the kind detected
  vdp --env prd release promote release/2026_DROP05

  # Promote after reading and accepting what the reset to the merge-base drops
  vdp --env prd release promote release/2026_DROP05 --kind new --accept-losses";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_BRANCH_LONG: &str = "\
The release branch to promote, e.g. `release/2026_DROP05`. It must exist on origin and
be one of the newest two release branches (see --allow-old-release).";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_KIND_LONG: &str = "\
The kind of promotion. `auto` (the default) decides by ancestry: a patch when main
already has part of this release branch, a new release when it has nothing of it.
`new` resets main to the merge-base first, `patch` only merges: use them to override
the detection.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_ACCEPT_ROLLBACK_LONG: &str = "\
Main already has a newer release than this branch: promoting this one rolls main back
to an older release. Without this flag that is refused.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_ACCEPT_LOSSES_LONG: &str = "\
A new promotion resets main, which drops the commits only main has (listed by
--dry-run). Without this flag they stop the promotion; with it they are dropped.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_ALLOW_OLD_LONG: &str = "\
Skip the scope check, which allows promoting only one of the newest two release
branches.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_PROMOTE_DRY_RUN_LONG: &str = "\
Print what would happen (kind, rollback, dropped commits, steps) and change nothing:
main is not reset, merged or pushed.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_BRANCHES_LONG_ABOUT: &str = "\
List the release branches on origin, newest first.

Fetches origin first (a failed fetch is a warning, and local refs are shown), then prints one line
per branch matching [release].branch_prefix: its name, the first 10 characters of its head and
`on main` when it has been promoted. Branches are ordered by fix version, numerically (DROP10 is
after DROP5), not as strings. Changes nothing.

Reads: [release], git.";

#[cfg(feature = "release")]
pub(crate) const RELEASE_BRANCHES_AFTER_LONG_HELP: &str = "\
Examples:
  # The release branches, newest first
  vdp release branches

  # Only the newest two, the ones the release tools can still change
  vdp release branches | head -2";

// ---- jira -------------------------------------------------------------------------------------------------------

#[cfg(feature = "release")]
pub(crate) const JIRA_LONG_ABOUT: &str = "\
Jira tickets.

Reads [jira] (base_url, user_secret, token_secret, advance_transition) and the Jira credentials
from the secrets (JIRA_USER and JIRA_TOKEN by default). The credentials are never prompted for.";

#[cfg(feature = "release")]
pub(crate) const JIRA_ADVANCE_LONG_ABOUT: &str = "\
Move tickets to their next status (`update_ticket.py`).

Each ticket is read, the transition is picked and performed, and the ticket is read again to show
its new status. The transition is --transition, else [jira].advance_transition (both matched
without regard to case against the transition names and the statuses they lead to), else the first
transition Jira offers for that ticket. A ticket that cannot be moved (unknown, no such transition,
a Jira error) is reported and does not stop the others.

Reads: [jira], Jira. Changes: the status of the tickets in Jira.
--dry-run resolves the transitions and prints `would advance ...` lines without changing anything.

Exit codes: 1 when some tickets could not be advanced; 2 when Jira cannot be reached or refuses the
credentials (the batch stops at once; tickets before it stay moved).";

#[cfg(feature = "release")]
pub(crate) const JIRA_ADVANCE_AFTER_LONG_HELP: &str = "\
Examples:
  # Preview the move of two tickets
  vdp --env prd jira advance --tickets INTEL-9876 INTEL-8765 --dry-run

  # Move them with the default transition
  vdp --env prd jira advance --tickets INTEL-9876 INTEL-8765

  # Move one ticket with a named transition
  vdp --env prd jira advance --tickets INTEL-9876 --transition \"Ready for release\"";

#[cfg(feature = "release")]
pub(crate) const JIRA_ADVANCE_TICKETS_LONG: &str = "\
The ticket keys, separated by spaces: `--tickets INTEL-9876 INTEL-8765`. At least one
is needed. All values up to the next option are tickets.";

#[cfg(feature = "release")]
pub(crate) const JIRA_ADVANCE_TRANSITION_LONG: &str = "\
The name of the transition (or of the status it leads to) to perform. Default:
[jira].advance_transition; when that is empty, the first transition Jira offers for
the ticket.";

#[cfg(feature = "release")]
pub(crate) const JIRA_ADVANCE_DRY_RUN_LONG: &str = "\
Resolve the transition of each ticket and print what would change, without moving any
ticket.";

// ---- sql --------------------------------------------------------------------------------------------------------

#[cfg(feature = "sql")]
pub(crate) const SQL_LONG_ABOUT: &str = "\
Ad-hoc SQL against the dbt target's database.

Connects with the dbt profile's target (--target, --profiles-dir; the profile is the dbt project's)
over SingleStore's ADBC driver, with the environment's secrets.";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_LONG_ABOUT: &str = "\
Run SQL files and print their results.

Each file is read whole and sent to the database as it is: the statements run with the rights of
the profile's user, whatever they are (the toolkit does not make them read-only). When a file holds
several statements the last result set is shown, as a table of at most --limit rows. With
--csv-dir the whole result (not just --limit rows) is also saved as `<file stem>.csv`, creating
the directory.

Reads: the SQL files, the dbt profile, the secrets. Changes: whatever the SQL changes, and the
CSV files.
--dry-run checks that the files exist and prints what would run and which CSV would be saved,
without connecting.

Exit codes: a SQL or connection error stops the run with exit code 2 (files before it have run).";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_AFTER_LONG_HELP: &str = "\
Examples:
  # Run a query and show the first 50 rows
  vdp --env dev sql run queries/orders.sql

  # Show 200 rows and keep the full results as CSV files
  vdp --env dev sql run queries/orders.sql queries/customers.sql --limit 200 --csv-dir out

  # Check the files without connecting
  vdp sql run queries/orders.sql --csv-dir out --dry-run";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_FILES_LONG: &str = "\
One or more SQL files, run in the order given. Each may hold several statements; the
result set of the last one is shown. A file that does not exist is an error.";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_CSV_DIR_LONG: &str = "\
Also save each result, in full, as `<file stem>.csv` in this directory (created when
missing). `orders.sql` gives `orders.csv`; two files with the same stem overwrite each
other.";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_LIMIT_LONG: &str = "\
The number of rows printed per result. It does not limit the CSV files.";

#[cfg(feature = "sql")]
pub(crate) const SQL_RUN_DRY_RUN_LONG: &str = "\
Check that the files exist and print what would run (file name and size) and which CSV
file would be saved. It does not connect to the database or write any file.";

#[cfg(feature = "sql")]
pub(crate) const SQL_EXPORT_LONG_ABOUT: &str = "\
Save a table as CSV.

Runs `SELECT * FROM <database>.<table>` (the name is validated and quoted) and writes every row,
with a header, to the output file, creating its directory. Prints the number of rows saved.

Reads: the table, through the dbt profile's target. Changes: the CSV file only.

Exit codes: an invalid table name, SQL or connection error is an error: exit code 2.";

#[cfg(feature = "sql")]
pub(crate) const SQL_EXPORT_AFTER_LONG_HELP: &str = "\
Examples:
  # Save a table to data/dev_landing_orders.csv
  vdp --env dev sql export dev_landing.orders

  # Choose the file
  vdp --env dev sql export dev_landing.orders --out exports/orders.csv";

#[cfg(feature = "sql")]
pub(crate) const SQL_EXPORT_TABLE_LONG: &str = "\
The table to export as `database.table`, e.g. `dev_landing.orders`. Names are quoted
for you; no other SQL is accepted here (use `sql run` for a query).";

#[cfg(feature = "sql")]
pub(crate) const SQL_EXPORT_OUT_LONG: &str = "\
The CSV file to write. Default: `data/<database>_<table>.csv` (dots in the name become
`_`). An existing file is overwritten.";

// ---- qa ---------------------------------------------------------------------------------------------------------

#[cfg(feature = "qa")]
pub(crate) const QA_LONG_ABOUT: &str = "\
Quality checks on the dbt manifest.

Both checks read the project's manifest (`target/manifest.json`) and run `dbt parse` first when
there is none (with the environment's secrets and --target, --profiles-dir). They change nothing
in the database.";

#[cfg(feature = "qa")]
pub(crate) const QA_ORPHAN_TABLES_LONG_ABOUT: &str = "\
Table models that nothing (tests aside) reads.

Lists the models materialized as tables that no other node of the project depends on. Tests do
not count as readers, so a table with only a test on it is an orphan. Prints the count and one
line per model.

Reads: the dbt manifest. Changes: nothing but a `dbt parse` when there is no manifest. Exit code 0
whether or not orphans are found; 2 when `dbt parse` fails.";

#[cfg(feature = "qa")]
pub(crate) const QA_ORPHAN_TABLES_AFTER_LONG_HELP: &str = "\
Examples:
  # List the orphan table models
  vdp --env dev qa orphan-tables

  # Keep the list
  vdp --env dev qa orphan-tables > orphan_tables.txt";

#[cfg(feature = "qa")]
pub(crate) const QA_VIEW_CANDIDATES_LONG_ABOUT: &str = "\
Views whose downstream tables take long to build: candidates to materialize.

For every view that has downstream models and no `t_<view>` table counterpart, adds up the build
times of the tables downstream of it (following views until the first table on each branch), from
the latest run recorded in the run-results table, and ranks the views by that total. Prints the ten
with the longest time (seconds, view name, number of downstream models) and writes all candidates,
with a dbt selector for each, to a CSV file. With none it prints a message and writes no file.

Reads: the dbt manifest; [qa].run_results_table (default `vdp_mgmt.dbt_run_results`, with `{env}`
and `{env_short}` placeholders such as `{env_short}_vdp_mgmt.dbt_run_results` for `dev_...`),
through the dbt profile's target, unless --no-timings.
Changes: the CSV file only.

Exit codes: 2 when the run-results table cannot be read (the message suggests [qa] or --no-timings).";

#[cfg(feature = "qa")]
pub(crate) const QA_VIEW_CANDIDATES_AFTER_LONG_HELP: &str = "\
Examples:
  # Rank the views by the build time of what depends on them
  vdp --env dev qa view-candidates

  # Without database timings (all times are 0), to a chosen file
  vdp --env dev qa view-candidates --no-timings --out candidates.csv";

#[cfg(feature = "qa")]
pub(crate) const QA_VIEW_CANDIDATES_OUT_LONG: &str = "\
The CSV file to write, relative to the current directory. Default:
`dbt_view_candidates.csv`. It is only written when there are candidates, and an
existing file is overwritten.";

#[cfg(feature = "qa")]
pub(crate) const QA_VIEW_CANDIDATES_NO_TIMINGS_LONG: &str = "\
Do not read run timings from the database: no connection is made and every time is 0,
so the candidates stay in the order of the manifest.";

// ---- env --------------------------------------------------------------------------------------------------------

pub(crate) const ENV_LONG_ABOUT: &str = "\
Inspect the resolved environment.

What the toolkit sees before a command runs: which environment was selected and how, which project
and configuration files apply, and which secrets are available. Read-only.";

pub(crate) const ENV_SHOW_LONG_ABOUT: &str = "\
Show the environment, project, configuration sources and secret keys (never values).

Prints the selected environment, the dbt project directory, the configuration files that were
read (in layering order), the CA certificate (and whether it is missing), the Vault URL and what
the Vault login did, the secret sources in precedence order, and every secret key with the source
it comes from. The values are never printed.

Reads: configuration, the environment, the secrets (this logs in to Vault unless --no-vault).
Changes: nothing.";

pub(crate) const ENV_SHOW_AFTER_LONG_HELP: &str = "\
Examples:
  # What `--env dev` resolves to, and where each secret comes from
  vdp --env dev env show

  # Without logging in to Vault
  vdp --no-vault env show";

pub(crate) const ENV_DOCTOR_LONG_ABOUT: &str = "\
Check the dbt engine, its SingleStore driver, the CA certificate, Vault and the dbt profile.

Prints one line per check, `ok` or `FAIL` with the detail: the dbt engine binary and its version,
the argument grammar (embedded or derived from the engine), the SingleStore ADBC driver,
DBT_ALLOW_EXPERIMENTAL_ADAPTERS (the SingleStore adapter is experimental in dbt v2), the CA
certificate of the environment, the dbt project, and the Vault login. It does not connect to the
database.

Reads: configuration, the environment, the secrets. Changes: nothing.

Exit codes: 1 when any check fails.";

pub(crate) const ENV_DOCTOR_AFTER_LONG_HELP: &str = "\
Examples:
  # Check the toolchain for the dev environment
  vdp --env dev env doctor

  # In a script: fail the job when something is off
  vdp --env prd --no-vault env doctor || echo \"toolchain problem\"";

// ---- dev --------------------------------------------------------------------------------------------------------

pub(crate) const DEV_LONG_ABOUT: &str = "\
Maintenance of the toolkit itself.

Commands for developing `vdp`, not for working on a dbt project.";

pub(crate) const DEV_GRAMMAR_LONG_ABOUT: &str = "\
Derive dbt's command-line grammar from a dbt binary (for crates/vdp-dbt/grammar/dbt.json).

The dbt wrapper reads dbt's arguments with a grammar of every command and flag (which flags take a
value, which are aliases). This derives it from a dbt binary by running `dbt completions fish` and
`dbt <command> --help`, and prints it as JSON, to refresh the copy embedded in `vdp`.

Reads: the dbt binary (--dbt, else the engine located as for `vdp dbt`; [dbt].binary). It runs that
binary. Changes: nothing, or the file given with --out.";

pub(crate) const DEV_GRAMMAR_AFTER_LONG_HELP: &str = "\
Examples:
  # Print the grammar of the configured engine
  vdp dev grammar

  # Refresh the embedded copy from a specific dbt build
  vdp dev grammar --dbt /opt/dbt/target/release/dbt --out crates/vdp-dbt/grammar/dbt.json";

pub(crate) const DEV_GRAMMAR_DBT_LONG: &str = "\
The dbt binary to derive the grammar from. Default: the engine located as for `vdp
dbt` ([dbt].binary, else $VDP_DBT_BIN, else `dbt` on PATH).";

pub(crate) const DEV_GRAMMAR_OUT_LONG: &str = "\
Write the JSON to this file instead of stdout. An existing file is overwritten.";

// ---- global options ---------------------------------------------------------------------------------------------

pub(crate) const GLOBAL_ENV_LONG: &str = "\
The environment to work on: LDV, ABT, CBT, PRD, or a name configured as
[environments.<name>]. `dev`, `val` and `prd` are aliases of LDV, ABT and PRD (CBT has
none). Default: $DBT_DEPLOY_TARGET, else [core].default_environment (LDV unless
configured). It selects the environment's settings, Vault paths and CA certificate,
and the `{env}` (`ldv`) and `{env_short}` (`dev`) placeholders of the configuration.
An unknown name is an error (exit code 2).";

pub(crate) const GLOBAL_PROJECT_DIR_LONG: &str = "\
The dbt project directory, the one with `dbt_project.yml`. Default: the nearest parent
of the current directory that has one. Its `vdp.toml` and its dotenv file
([core].dotenv, default `.devcontainer/.env`) are read.";

pub(crate) const GLOBAL_CONFIG_LONG: &str = "\
An extra TOML configuration file, layered over the built-in defaults,
/etc/vdp/vdp.toml and the project's vdp.toml (later files win, key by key). Unlike
those, it must exist.";

pub(crate) const GLOBAL_TARGET_LONG: &str = "\
The dbt target the toolkit itself uses, for its database connections and its own dbt
runs (`dbt parse`, the release validation). Default: the profile's `target`. It does
not change what `vdp dbt ...` passes to dbt.";

pub(crate) const GLOBAL_PROFILES_DIR_LONG: &str = "\
The directory with profiles.yml. Default: the first of $DBT_PROFILES_DIR, the dbt
project directory and ~/.dbt that has a profiles.yml.";

pub(crate) const GLOBAL_NO_VAULT_LONG: &str = "\
Do not log in to Vault: secrets come from the project's dotenv file and the process
environment only. As $VDP_NO_VAULT it takes a boolean: `1`, `true`, `yes` or `on` turn
it on; `0`, `false`, `no` or `off` leave it off.";

pub(crate) const GLOBAL_VERBOSE_LONG: &str = "\
Debug logging on stderr (and in the --log-json file). $VDP_LOG, when set, overrides
it.";

pub(crate) const GLOBAL_QUIET_LONG: &str = "\
Log warnings and errors only on stderr. The --log-json file still gets info events.";

pub(crate) const GLOBAL_LOG_JSON_LONG: &str = "\
Also write the log as NDJSON, one event per line (timestamp, level, target, message),
to this file, overwriting an existing file (also `$VDP_LOG_JSON`). It gets `info` and
above by default, `debug` and above with `-v`, whatever `-q` says; `$VDP_LOG`
overrides both sinks. Not used by `vdp dbt`.";
