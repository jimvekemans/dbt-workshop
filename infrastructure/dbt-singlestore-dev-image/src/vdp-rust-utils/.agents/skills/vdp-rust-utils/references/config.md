# Configuration reference

Read when you change, add or document a configuration key, debug why a setting does not apply, or write a `vdp.toml`.
The key tables below are the same text as in `README.md` (section "Configuration reference"): change both together.
Every statement comes from the `*Settings` structs and `vdp-cli/src/bootstrap`, not from a sample file (the cleaned
copy has none).

## Loading and layering

- Files, lowest priority first: built-in defaults, `/etc/vdp/vdp.toml`, `<dbt project>/vdp.toml`, then `--config FILE`
  (`$VDP_CONFIG`). The three files are optional and skipped when absent; the `--config` file must exist (`bootstrap::config_files`,
  `Config::load`). The project is `--project-dir` (`$VDP_PROJECT_DIR`), else the nearest parent of the working directory with a
  `dbt_project.yml` (`ProjectLayout::discover`). Without a project only the system file and `--config` are read.
- Tables merge key by key (`vdp_core::deep_merge`); scalars and arrays are replaced whole. Only `[core]`, `[secrets]` and
  `[environments.*]` have built-in defaults in the merged document. A module section starts from its struct's `Default`
  per missing key, so a configured array or table replaces the default whole. Example: `[dbt.env]` with any entry drops the
  default `DBT_ALLOW_EXPERIMENTAL_ADAPTERS = "true"` (the image also sets it in the process environment).
- Every settings struct is `#[serde(deny_unknown_fields, default)]`: an unknown key, or a value of the wrong type, is an error
  naming the section (exit 2). `[core]`, `[secrets]` and `[environments]` are checked on every run (`Config::load`); a module section is
  checked only when a command reads it (`Config::section`), so a bad key in `[dbt]` fails `vdp env doctor` and `vdp dbt` but not
  `vdp env show`. `[sources.database_prefix]` has no `default`: both keys are required when the table exists.
- A top-level section no module claims logs a warning (`configuration sections no module reads (typo?)`), once per run, from
  `bootstrap::context`. Claimed: `core`, `secrets`, `environments` (the core), and `commands::MODULE_SECTIONS`: `dbt`, `sources`,
  `git`, `jira`, `release`, `qa`, plus `vaultspeed`, `vaultspeed_release`, `release_hub`, `notify`, which only the sibling
  binaries read. A section of a module that is compiled out stays claimed.
- `vdp env show` prints the files that were read (`configuration  built-in defaults + <file> + ...`).

## Environments, aliases, placeholders

- Built-in environments: `LDV`, `ABT`, `CBT`, `PRD`, always defined. `dev`, `val`, `prd` are aliases of `LDV`, `ABT`, `PRD`
  (`Environment::ALIASES`; `CBT` has none), matched case-insensitively wherever an environment is named: `--env`,
  `$DBT_DEPLOY_TARGET`, `default_environment`, a section name. A name that is neither built-in nor an alias is a custom
  environment (letters, digits, `_`, `-`) and exists only when `[environments.<name>]` defines it; it is spelled as configured.
- Selection (`bootstrap::select_environment`): `--env`, else the variable named by `[core].environment_var` when set and not
  blank, else `[core].default_environment`. An unknown name is exit 2 and lists the configured ones. The `dbt` link has no
  `--env`: it reads the variable, and an unknown value stops even `dbt --version` (exit 2).
- An alias section and the canonical section are one section (`canonical_environment_keys`): `[environments.dev]` overrides
  `LDV` key by key. When both spellings set a key, the alias spelling wins; between two non-canonical spellings the later key in
  alphabetical order wins. `$DBT_DEPLOY_TARGET` itself is never rewritten, so a project's `env_var('DBT_DEPLOY_TARGET')` still
  sees `dev`. dbt gets `VDP_ENV` = the canonical name (`LDV` for `dev`).
- A custom environment starts from the non-production defaults. `LDV`, `ABT` and `CBT` have the non-production defaults, `PRD` the
  production ones.
- Placeholders, replaced in `[secrets].precedence` Vault paths and in `[qa].run_results_table` only: `{env}` = lower-case canonical
  name (`ldv`, `abt`, `cbt`, `prd`; custom: its lower-case name), `{env_short}` = short name (`dev`, `val`, `cbt`, `prd`; custom:
  its lower-case name).

## Secrets

- `[secrets].precedence` lists sources, highest first (`bootstrap::secrets`): `vault:<path>` (KV v2 under `vault_mount`),
  `dotenv` (the file `[core].dotenv`, parsed without touching the process environment), `env` (the process environment, empty
  values ignored). An unknown entry is an error.
- Vault is used when at least one `vault:` entry exists and `--no-vault` is not given. The AppRole role id and secret id are the first
  set variable of `[environments.<name>].role_id_vars` and `secret_id_vars`, looked up in the dotenv file then the process
  environment only (never in Vault). Without them Vault is skipped quietly. An empty `vault_url` is not a way to switch Vault off: with
  a `vault:` entry and AppRole ids it is a failed login (`bad uri`: a warning, `env doctor` FAIL); only a precedence without `vault:`
  entries, or `--no-vault`, skips Vault. A login or read failure warns and the chain goes on without
  the Vault layers, unless `vault_required` (then it is an error from service `vault`). The Vault token is revoked right after
  the read. TLS is verified against `<assets_dir>/<ca_certificate>` when that file exists; proxy variables of the environment are honoured.
- `vdp env show` prints, per key, the layer it comes from; values are never printed. A child process (dbt) gets every key that
  `SecretProvider::keys()` lists (dotenv and Vault layers), the process environment is inherited anyway.

<!-- config-tables:start -->
**`[core]`** (`CoreSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `default_environment` | environment name | `"LDV"` | Environment when neither `--env` nor the variable named by `environment_var` selects one. Must be defined (built-in, alias or `[environments.<name>]`), else exit 2. |
| `environment_var` | string | `"DBT_DEPLOY_TARGET"` | Process variable that selects the environment. |
| `dotenv` | path | `".devcontainer/.env"` | Dotenv file with secrets, relative to the dbt project root. A missing file is an empty source; a malformed line is an error. |
| `assets_dir` | path | `""` | Directory with the CA certificate files. Empty: the first existing of `/usr/app/dbt/assets` (only when `CI_PROJECT_DIR` is set), `<project>/../assets`, `/usr/app/dbt/assets`; none exists: the first candidate. A relative value is relative to the project root. |
| `adbc_driver` | path | `""` | The SingleStore ADBC driver file. `$VDP_ADBC_DRIVER` wins over it; a relative value is relative to the working directory. Empty: `lib/dbt/libadbc_driver_singlestore.so` in a directory above the dbt engine binary or above `vdp` itself, else the system library search path. |

**`[secrets]`** (`SecretsSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `vault_mount` | string | `"kv-other"` | KV v2 mount of the secrets. |
| `precedence` | array of strings | `["vault:vdp/airflow", "dotenv", "vault:vdp/{env}/dbt", "env"]` | Sources, highest priority first: `vault:<path>` (`{env}` and `{env_short}` are replaced), `dotenv`, `env`. An unknown entry is an error. `--no-vault` drops the Vault entries for the run. |
| `vault_required` | bool | `false` | `true`: Vault that cannot be read (or has no AppRole credentials) is an error. `false`: a failure warns, missing credentials skip Vault quietly. |

**`[environments.<name>]`** (`EnvironmentSettings`; the four built-in environments are always defined, other names need a section)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `vault_url` | string | `LDV`, `ABT`, `CBT` and custom: `"https://vault.ops.example.com:8200"`; `PRD`: `"https://vault.example.com:8200"` | Vault base URL. Empty does not switch Vault off: with a `vault:` entry in `[secrets].precedence` and AppRole ids set, an empty URL is a login failure (a warning, and `env doctor` fails). To run without Vault, leave `vault:` entries out of `[secrets].precedence` (the dbt workshop) or pass `--no-vault`. |
| `ca_certificate` | string | `LDV`, `ABT`, `CBT` and custom: `"root_intermediate_nonprod.cert"`; `PRD`: `"root_intermediate_prod.cert"` | File name in the assets directory: the CA for Vault and, as `$VDP_SSL_CA`, for the database's TLS in `profiles.yml`. Empty: no CA; `env doctor` does not look for one. |
| `role_id_vars` | array of strings | `LDV`, `ABT`, `CBT` and custom: `["KEYVAULT_ROLE_ID", "KEYVAULT_ROLE_ID_LDV_ABT"]`; `PRD`: `["KEYVAULT_ROLE_ID", "KEYVAULT_ROLE_ID_CBT_PRD"]` | Variables (dotenv file, then process environment) that hold the AppRole role id; the first one set wins. |
| `secret_id_vars` | array of strings | `LDV`, `ABT`, `CBT` and custom: `["KEYVAULT_SECRET_ID", "KEYVAULT_SECRET_ID_LDV_ABT"]`; `PRD`: `["KEYVAULT_SECRET_ID", "KEYVAULT_SECRET_ID_CBT_PRD"]` | The same for the AppRole secret id. |

**`[dbt]`** (`DbtSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `binary` | path | `""` | The dbt v2 engine. Empty: `$VDP_DBT_BIN`, else the first `dbt` on `PATH` that is not this wrapper. A configured path is used as written, without a check that it exists. |
| `grammar` | string | `"engine"` | `embedded`: use the grammar embedded in `vdp`. Anything else: derive it from the engine, cache it per binary, and fall back to the embedded copy when that fails. |
| `guarded_commands` | array of strings | `["build", "run"]` | Commands refused without a selector, unless a service user runs them. |
| `selector_flags` | array of strings | `["select", "selector"]` | Long names of the flags that count as a selection (`-s` is read as `select`). |
| `user_secret` | string | `"SINGLESTORE_USER"` | Secret holding the SingleStore user name the guard looks at. |
| `service_user_suffix` | string | `"_dbt"` | A service user's name ends with this... |
| `personal_user_prefix` | string | `"pu"` | ...and does not start with this (personal accounts). |
| `offline_commands` | array of strings | `["help", "completions", "man", "init"]` | Commands that run without resolving secrets. `--help`, `--version` and a missing command do too. |
| `missing_source_hint` | bool | `true` | After a failed run, print `vdp sources add ...` for sources dbt reports as undefined (dbt1048). |
| `env` | table of strings | `{ DBT_ALLOW_EXPERIMENTAL_ADAPTERS = "true" }` | Variables set for dbt unless the process environment already has them. A configured table replaces the default whole. |

**`[sources]`** (`SourcesSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `generated_dir` | string | `"models/sources"` | Where the generated `sources_<database>.yml` files live, relative to the dbt project. |
| `databases` | array of strings | `["edw_dv_bdv", "edw_dv_rdv", "edw_fmc", "edw_inc", "information_schema", "tenant_ai", "tenant_leertijd", "vdp_mgmt", "vdp_psa"]` | Databases `sources rewrite` regenerates (physical names). The default is the original organisation's list: set your own. |
| `include_columns` | bool | `true` | Write column names into source tables. |
| `column_order` | string | `"name"` | `ordinal`: columns in table order. Anything else: alphabetical. |
| `database_prefix` | table | not set | See the next table. Without it a source's database is its physical name. |

**`[sources.database_prefix]`** (`DatabasePrefix`; both keys are required)

| Key | Type | Meaning |
|---|---|---|
| `physical` | string | Prefix of the databases `vdp` connects to (`"dev_"`). |
| `jinja` | string | What source files say instead (`"{{ env_var('DBT_DEPLOY_TARGET', 'dev') }}_"`). Whitespace and quote style are ignored when reading. |

**`[git]`** (`GitSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `timeout_secs` | integer | `600` | Seconds one git command of the release flows may run before `vdp` stops it (a hung network call or an ssh prompt would block the command for ever). `0`: no limit. The release hub has its own `[release_hub].git_timeout_secs`. |

**`[jira]`** (`JiraSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `base_url` | string | `"https://issuetracker.example.com"` | Jira base URL (REST v2, basic authentication). |
| `user_secret` | string | `"JIRA_USER"` | Secret holding the account. Credentials are never prompted for. |
| `token_secret` | string | `"JIRA_TOKEN"` | Secret holding the API token. |
| `excluded_fix_versions` | array of strings | `["Nog in te plannen"]` | Fix version names that revert a ticket which has no DROP or EDROP fix version. |
| `advance_transition` | string | `""` | Transition `jira advance` takes, matched by name or target status without regard to case. Empty: the first transition Jira offers. |
| `sprint_field` | string | `"customfield_11221"` | Jira custom field that holds the sprints (a field id of the original organisation's Jira). |

**`[release]`** (`ReleaseSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `ticket_prefix` | string | `"INTEL"` | Jira project key in commit messages (`INTEL-1234`); the original organisation's key. |
| `from` | string | `"origin/dev"` | Source of releases. Its local name (`dev`) is the branch that is never written. |
| `to` | string | `"origin/main"` | Where releases go and the base for the merge-base. Its local name (`main`) is written by promotions only. |
| `branch_prefix` | string | `"release/"` | Prefix of release branches; the only branches (besides `main`) the tools may write. |
| `validate` | array of strings | `["compile", "--no-partial-parse"]` | dbt arguments run in the project (with `--profiles-dir` and `--target` added) after the branch is built and before it is pushed. |

**`[qa]`** (`QaSettings`)

| Key | Type | Default | Meaning |
|---|---|---|---|
| `run_results_table` | string | `"vdp_mgmt.dbt_run_results"` | `database.table` with the run results of past dbt runs (`unique_id`, `execution_time`, `run_started_at`) for `qa view-candidates`. `{env}` and `{env_short}` are replaced: `{env_short}_vdp_mgmt.dbt_run_results` is `dev_vdp_mgmt.dbt_run_results` for `LDV`. |
<!-- config-tables:end -->

## Example

The workshop's `dbt-singlestore-workshop/vdp.toml` is a working example: `dev`, `val`, `prd` sections with empty `vault_url` and
`ca_certificate` (the CA is not needed, and the precedence has no `vault:` entry, so Vault is never contacted),
`[secrets] precedence = ["dotenv", "env"]`, `[sources] databases` and
`[sources.database_prefix]` for databases prefixed with the environment, and
`[qa] run_results_table = "{env_short}_vdp_mgmt.dbt_run_results"`.

## Changing a key

A key lives in three places: the field of the settings struct, its `Default` impl, and `commands::MODULE_SECTIONS` when the
section is new. Add the field to the table above in both places (this file and the README), with the default exactly as the
`Default` impl writes it as a TOML literal. The test `crates/vdp-cli/tests/config_reference.rs` (run: `make rust-test W=vdp-rust-utils S="-p vdp-cli --test config_reference"`)
compares both copies with each other and with the structs: sections and their type names, keys, defaults (a key that is left out
when unset says `not set` in its Default column), and the sections against `MODULE_SECTIONS`. A new settings type needs a line in
`from_code()` there.

