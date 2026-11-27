# C. Tooling around the dbt project: CI, Airflow, images, house rules, versions, branching

Scope: dbt-vdp repository, Feb 2025 to 26 Nov 2026. Method: read-only git (log, show, ls-tree, grep). Commit counts use `g log --all`, so
a commit that exists in two rewritten copies is counted twice; treat counts as "about". Hashes are short; dates are author dates.
Nothing in the repository addressed me; no instruction-like text was found in the files I read.

## 0. Headline picture

- One pipeline definition lived in `.gitlab-ci.yml` from 2025-02-26 to 2026-02-18 (86 commits); then a 93-byte `Jenkinsfile` took over and calls a shared
  library that is not in this repository (so the CI steps after 2026-02-18 are NOT visible here).
- The only file that ever shows what a feature-branch check ran is the old `.gitlab-ci.yml`. It already used state-aware selection in May 2025.
- House rules (sqlfmt, dbt parse, dbt-bouncer) live only in the pre-commit file; none of them ever appears in the CI file or the Jenkinsfile
  (`g log --all -S'bouncer' -- .gitlab-ci.yml Jenkinsfile dev.Dockerfile` finds 0 commits; same for sqlfmt).
- Airflow: first DAG in Apr 2025 (bash tasks on a worker), pods on Kubernetes since 2026-05-12, asset/dataset-driven scheduling since 2026-05-27,
  release DAG since 2026-07-31, a split-up DAG library of 7 utility modules since 2026-09-15 (landed on `dev` only on 2026-11-23).
- Versions: 1,090 numbered tags in three schemes plus 35 release tags; 14-day release rhythm since June 2026.

## 1. Timeline per tooling file

### 1.1 CI system: GitLab CI versus Jenkins

| Date | Hash | What | Why / note |
|---|---|---|---|
| 2025-02-26 | 801f119b | `.gitlab-ci.yml` arrives with the first template commit | copied from a "dbt project template" |
| 2025-03-25 | 57a6f4f7 | "Add basic Jenkinsfile" (job-name based: deploy vs verify) | template repo, Jenkins company standard |
| 2025-03-27 | b930f391 | "Remove gitlab-ci.yaml because it conflicts with Jenkins" | one CI at a time |
| 2025-04-10 | 0fb77294 | "Revert Jenkinsfile" (20 lines removed) | Jenkins attempt dropped |
| 2025-05-02 | 81ab1595 | "Setup basic GitLab CI file"; 05-10 f751d5a1 "Try-out version of deploy pipeline"; 05-12 about 12 commits fixing tag creation | GitLab CI becomes the pipeline; first numbered tags v1.0.x on 2025-05-10..12 |
| 2025-10-06 | f7c082d2 / 13411ab2 | Jenkinsfile with `@Library(jenkins-vdab-dbt) dbtPipeline()`; removes `.old.Jenkinsfile` and `.gitlab-ci.yml` | on a side branch only (not in any current ref); reverted 2025-10-21 (32e13460) |
| 2025-12-04 | be139fb7, 4cf9eb32 | "Setup reworked CI pipelines following Git Flow logic": `.old.Jenkinsfile`, `.deploy.gitlab-ci.yml` and the deploy-process drawing deleted | GitLab CI is now the full Git Flow pipeline (see 1.2) |
| 2026-01-23 | 1f3b8f5f | "Removed .gitlab-ci" (249 lines) on a side branch; first tag without "v" (1.254.0) the same day | start of the Jenkins era (inferred) |
| 2026-01-30 | 66975329 | `dev.Dockerfile` and `docs.Dockerfile` added (see 1.5): Jenkins builds the images | |
| 2026-02-18 | 343fdd6c | "Squashed commit" onto `dev`: GitLab CI gone, `Jenkinsfile` = shared library call | one squash combines the removal and both Dockerfiles |
| 2026-10-07 | 5ad25bbd..0ffb872d | six commits "testing" on branch `YCL_activate_sonar`: own Jenkinsfile with a unit-test stage (`dbt test --selector critical_tests`) and a Sonar stage (`abortPipeline: false`) | experiment, never merged |
| 2026-11-26 | d3d1b4fa | Jenkinsfile now points to the library branch `feature/pipeline-union` (INTEL-26600) | pipeline in development, same day as DROP26 |

Counts: `.gitlab-ci.yml` 86 commits, `Jenkinsfile` 21, `.old.Jenkinsfile` 4. Current state: the Jenkinsfile is two lines; everything it does lives outside the repo.
The Mar-Apr 2025 Jenkinsfile set `VERSION_BASE = '1.0'` and decided stages by Jenkins job name (deploy vs build), which matches the later `v1.0.x` start.

### 1.2 What the GitLab CI file ran (versions read)

- 2025-05-12 (c62405cc): stages verify_feature_ldv, create_release, deploy_ldv, deploy. Verify (merge request into `main` only): compile the target branch as a
  baseline, compile the source branch, then `test --select state:modified --state <target dir>`. Release: tar archive, tag `v1.0.N`, push the tag. Deploy: a Python script copied the DAGs for tag N to Airflow.
- 2025-06-29 (d8a8017d): `state:modified` becomes `state:modified+` ("Fix state:modified+": also children of changed models).
- 2025-07-27 (19645c05): tag logic made "feature merge = minor bump, bugfix = patch bump"; check applies to feature and bugfix branches (2b606645 2025-07-28 drops the branch-type check).
- 2025-08-26 (87d78073 "squash", 496673e2 "ci"): the check now does a real `build --select state:modified+ --state artifacts --defer` into a schema named after the branch (`SCHEMA: <branch with / and - replaced by _>`), on the `ci` target, then the old test run, then `run-operation drop_nodes` to clean the branch schema.
- 2025-10-15 (757c50e0): "Add verification run for release branches". 10-30 (c0c1b23a): drop_nodes step re-added.
- 2025-11-28 (de5d68f4): build excludes tests (`--exclude-resource-type test`); b1a64ce5 removes `--empty` "due to adapter bug"; 12-11 (5cf3460f) adds `--empty` back; 12-16 (d6410a99) removes it again "due to consistent bug".
- 2025-12-04 (be139fb7) final shape, 245 lines, three verify jobs:
  1. `verify_feature_to_dev` (merge request into dev): compile dev as baseline, merge dev into the feature, `build --select state:modified --exclude-resource-type test --state artifacts --defer --target=ci` into the branch schema, then drop_nodes.
  2. `verify_dev_to_release` (merge request into `release/*`): `build --select state:modified+ --state artifacts --defer --fail-fast`, schema `ci_release_verify`.
  3. `verify_release_to_main`: a full `build` (no selection) into schema `ci_cbt_verify`, then drop_nodes.
  Then `tag_dev` (push to dev: tag `vX.Y.Z`), `tag_main` (merge of a release into main: `<release>.0`, next push `.1`, ...), `publish_dev_artifacts`, `publish_main_artifacts`, deploy jobs per environment (LDV automatic on tag, ABT scheduled with the newest dev tag, CBT and PRD manual with the newest DROP tag) and a `changelog` job.
- 2026-01-08 (a42dca25): release tag pattern changes from `YYYYQn...` to `YYYY_DROPnn...` (the new naming; first DROP tag 2026_DROP03.0 on 2026-01-08).

### 1.3 Pre-commit, sqlfmt, formatting

| Date | Hash | Change |
|---|---|---|
| 2025-03-25 | 18c76b2d | "disable sqlfluff, use sqlfmt and other improvements" (`.sqlfluff` kept until 2026-06-23 b802efe2 cleanup) |
| 2025-03-28 | b26e6420 | `.pre-commit-config.yaml` + `.pre-commit-hooks.yaml`: first hook is dbt-bouncer |
| 2025-08-20 | fb70c3aa | hooks adapted to the new container layout |
| 2025-12-19/22 | 5d255919, 431bfd59 | "Project formatting": hooks sqlfmt (`sqlfmt models`, always run), `dbt compile`; one commit reformats 921 files (+18.9k/-16.3k lines); the second copy on dev changed 887 files. Merged to dev 2025-12-22 (70f83680) |
| 2026-06-09 | 2da70455 | hook sqlfmt runs on staged files only and re-adds them (`sqlfmt "$@" && git add`); `dbt compile` replaced by `dbt parse` |
| 2026-08-18 | f1a31e98 | branch `fmt`: one commit "format" (never merged) |
| 2026-09-09 | b01c0929 | branch `hotfix/project-yaml-force-formatting`: "[skip ci] Formatted yml files", 213 files (+1.9k/-1.7k) |

Earlier manual reformatting: 640c2cdc 2025-08-28 "Reformat files" (9 files). Commits whose subject mentions "format" on dev (non-merge): 23 in total, 7 of them in Nov 2025, the month before the mass reformat (`g log dev --no-merges -i --grep=format`).
Current state: three local hooks (sqlfmt on changed files, dbt parse, dbt-bouncer) that run only on a developer machine inside the dev container.

### 1.4 dbt-bouncer (`dbt-bouncer.yml`, 8 commits)

| Date | Hash | Rule change |
|---|---|---|
| 2025-03-28 | b26e6420 | one rule: `check_model_names` (pattern `_ab$ | _hb$`) on one indicator folder, severity warn |
| 2025-04-28 | 773563a8 | rule `check_model_depends_on_multiple_sources` on the information mart, warn |
| 2026-06-18 | 105d77cc | both warn rules removed; new `check_model_has_semi_colon`, `check_model_hard_coded_references`, and model names must match `^[a-z0-9_]+$` |
| 2026-08-11 | f340387b | semicolon rule tidied |
| 2026-08-19 | 7ec28296 | "check op verboden functies": regex rules forbidding `to_char(` and `str_to_date(` in models, message "FOUT: ... Check dbt style guide" |
| 2026-11-24 | 0af07434 | "fix bouncer rules + remove str_to_date": the regexes were too narrow (`\b` word boundary missed matches); widened to `.*to_char\s*\(` and `.*str_to_date\s*\(` |

Current state: four to five error-level rules, all about SQL text. The forbidden-function rule took 97 days (2026-08-19 to 2026-11-24) to be fixed after introduction (the word-boundary pattern let calls through), and the same commit removes `str_to_date` from 3 models (inferred: the corrected rule found hits).

### 1.5 Dev image, devcontainer, docs image, secrets scanning

Devcontainer (`.devcontainer/`, 58 commits, 2025-02-26 to 2026-10-07). What a developer gets: a VS Code container on the company dbt base image (version pinned from 2025-07-14 db71c6cd, "Pinned docker image versions"), with the dbt Power User extension, YAML/Jinja/Python extensions, SSH keys mounted and loaded into an agent, `dbt deps` run on first start, the project mounted as workspace.
How the database password arrives: `post_create.sh` (and before it `create_env_file.sh`, 2025-03) asks on first start "Please enter your PU-username" and "password for <user> (used for singlestore connection)", writes `.devcontainer/.env` (host, lower-cased user, password, default database `edw_dv_rdv`). `profiles.yml` reads them as environment variables and uses the user name as the schema: each developer builds into a schema with their own name (target `default`). "Skip env questions if file already exists": fba00ec8 2025-04-10.
Other changes: local SingleStore container option in compose (3b066a19 2025-05-10, removed 2497bf15 2026-04-29); 2025-08-13 (1863aafe) .env auto-loaded into bash; 2026-07-16 (357cef48, branch `dbt-image-separation`) "Base image herwerkt naar meer lightweight variant" (own `.devcontainer/Dockerfile`, 65 lines).

`profiles.yml` (28 commits): targets over time: `default` (own schema), `ci` (database `vdp_ci`, schema from the `SCHEMA` variable, 2025-08-26), `run` (schema from variable), `collibra` (2025-09-18, 816b7842), `reload` (2026-09-17, 762ddb4f). Always password from environment, never in the file.

`dev.Dockerfile` (58 commits, first 2026-01-30 cea20008): the production image "dbt-vdp" per environment. Build does: copy project, `dbt deps`, compile for the Collibra target, write build date/git version/commit into the docs overview page (c5dbc5df 2026-11-06), generate static docs, tar the `target` folder. Secrets (SSH key, vault role, SingleStore credentials) come from build-secret mounts.
Between Jan and May 2026 the image build ALSO built changed models in SingleStore: `wget` the artifacts tar of `PREVIOUS_VERSION`, then `build -s state:modified --state artifacts/target/dbt-docs --exclude-resource-type test`, then `drop_nodes` (version of fc430297^). Removed 2026-05-19: fc430297 "ENV builds to singlestore don't happen in CI anymore during deploy" and 160e92cf "Removed drop_nodes step".
`perform_database_maintenance` (macro added 072f7a4c 2026-03-03: grant maintenance, 141 lines, plus `sync_grants_with_singlestore`): added to the image build 2026-05-22 (c09071a8), also becomes the first step of the Airflow release DAG, and is removed from the image build again 2026-11-26 (d3d1b4fa, which also adds `--exclude resource_type:test` to the Collibra compile).
Trial-and-error editing: 39 commits titled "Edit dev.Dockerfile" (2026-02-27 3, 03-02 4, 03-10 13, 04-21 1, 04-24 3, 04-27 11, 08-06 3, 08-14 1), i.e. edited and rebuilt through the web editor until the image built. 23 numbered tags point at such commits (see 5).

`docs.Dockerfile` (6 commits, 2026-01-30 .. 2026-09-21): takes the static docs from the dev image and copies `index.html`, `catalog.json`, `manifest.json` into an nginx image (nginx base bumped 2026-09-18 d48d5d36). Header comment: Jenkins builds the dev image locally first and pushes only if the docs image builds; images are named per environment because an executor can run two builds at once. `docker-entrypoint.sh` (2026-03-02, 4a573cf8): `umask 000` then a shell.

Secrets scanning: `.gitleaks.toml` + `.gitleaksignore` both 2026-10-02 (4c21fdb0, 60eb4a97, 2 commits). Rules: extend the default rules, but ignore Data Vault key column names (`_hkey`, `_dhkey`, `_dakey`) and 32-character hex strings (MD5 hashes), which otherwise look like secrets. The ignore file lists two findings, both inside commit 3b066a19 (2025-05-10, the local SingleStore container commit): a test password for a local container in `profiles.yml` and in `compose.yaml`, "oude versie van een wachtwoord" (old version of a password).
Sonar: only the `YCL_activate_sonar` experiment above (`sonar-project.properties`, scan scope `models`; 2026-10-07, 7 commits including the config, not merged).

### 1.6 Other files

| File | First | Commits | Notes |
|---|---|---|---|
| `selectors.yml` | 2026-09-17 (762ddb4f) | 5 | `critical_tests` (unique-combination tests), `non_critical_tests`, `daily_load` (default; models only; excludes tags `on-demand` and `monthly`), monthly variants. Fix 432b08f9 2026-10-19. |
| `incrementeel_laden_initial_load.py` | 2026-09-08 (aea778aa) | 3 | helper that calls a backup/restore macro for the first (initial) incremental load of a model; touched 2026-09-25, 2026-11-05 ("dbt v2 changes"). |
| `tests/setup_tests.sh` | 2025-11-28 (01c4d206) | 5 | "script for testers": asks the branch, compiles the merge-base state, then `build -s state:modified --defer --state artifacts` in one of three modes: user prefix, `dev` prefix, branch-named schema (mode 3 added 2026-09-09 5ba23447 "Updated artifacts Way of Working"). |
| `tests/integration/test_dbt_vdp_k8s.py` | 2026-05-12 (cf48a9dc) | 1 | integration test that goes with the Kubernetes DAG. |
| `airflow/test_dag_parse.py` | 2026-10-02 (23c34d99) | 3 | checks that all DAG files parse (Airflow 2 and 3); deleted from the active list 2026-10-21 on the DAG branch, present in the known root list. |
| deleted tooling | | | `.devcontainer/compose.yaml`, `.devcontainer/vdab/*` (company Dockerfile, pip.conf), `python/*` wrapper/managers/key-vault scripts, `CHANGELOG.md` (2025-12-16 ced4ad4d), `VDAB_dbt_deploy_process.drawio` (2025-12-04), `dbt-workflow.md`, `.sqlfluff`, `pyproject.toml`, certificates in `assets/`. |

## 2. The verify step over time

| Period | What a feature branch check did | State-aware? | Tests? |
|---|---|---|---|
| 2025-03 template | nothing (CI removed, then Jenkinsfile reverted) | no | no |
| 2025-05-06 .. 06-28 | compile target branch for a baseline, compile branch, `test --select state:modified` (target `LDV`) | yes, `state:modified`, state = target-branch compile (fa9c0275 2025-05-06) | tests only, nothing built |
| 2025-06-29 .. 08-25 | same with `state:modified+` (d8a8017d) | yes, with children | tests only |
| 2025-08-26 .. 12-03 | `build --select state:modified+ --state artifacts --defer` into schema per branch on target `ci` (database `vdp_ci`), then the test run, then `drop_nodes` | yes + `--defer` | yes (build runs tests) |
| 2025-11-28 | `--exclude-resource-type test` added to the build | yes | no (data tests excluded for feature checks) |
| 2025-12-04 .. 2026-02 | three jobs: feature to dev (`state:modified`, no tests, schema per branch), dev to release (`state:modified+`, `--fail-fast`, schema `ci_release_verify`), release to main (full build, `ci_cbt_verify`) | yes for the first two; the third is a complete rehearsal | only in the full build |
| after 2026-02-18 | not visible: Jenkins shared library; only the experiment branch shows `dbt test --selector critical_tests` | unknown | unknown |

Answer to "when did state-aware selection start": 2025-05-06 (fa9c0275 "start of CI pipeline", first `state:modified` in `.gitlab-ci.yml`), i.e. in the first CI version; `--defer` since 2025-08-26 (87d78073). `--empty` (build with zero rows) flip-flopped three times: removed 2025-11-28, re-added 2025-12-11, removed 2025-12-16 ("consistent bug" in the adapter); never present when the file was removed.
Developers' own copy of this check is `tests/setup_tests.sh` (2025-11-28).

## 3. Airflow DAGs

| DAG / file | Introduced | Commits | How it runs dbt, schedule, failure, state |
|---|---|---|---|
| `dbt_swf_indicatoren.py` | 2025-04-28 (2a814ea3 "Renamed dbt dag") | 4 | first dbt DAG; removed 2025-08-26 (44e736cd "Removed old dag") |
| `dbt_vdp.py` (daily load) | 2025-05-23 (8295b3c0, 103 lines) | 103 | see below |
| `ai_triggers.py` | 2025-05-22 (7540bc2f "add temporary ai trigger DAGs") | 11 | one DAG per AI data asset, generated from an Airflow Variable mapping; scheduled on the dataset events that `dbt_vdp` emits; POSTs a trigger URL to the AI dispatch framework. Header: temporary stand-in for existing DAGs whose promotion to higher environments was blocked. |
| `dbt_release.py` | 2026-07-31 (b5a582ce) | 11 | see below |
| `dbt_reload.py`, `dbt_daily_load.py`, `dbt_vdp_new.py` | 2026-09-15..18 | 4/3/2 | only on `feature/dbt-reload-dag` |
| `utils/dbt_*.py` (7 modules) + `compat.py` | 2026-09-15 on the DAG branch; on `dev` 2026-11-23 | 2..5 each | pod templates, commands, state, logs, trigger helpers; `compat.py` for Airflow 2 versus 3 (Dataset versus Asset) |

`dbt_vdp.py` story:
1. 2025-05-23: bash tasks on the Airflow worker inside a virtual environment: generate sources, `source freshness`, a `.dbtignore` file built from an Airflow Variable `disabled_dbt_models` (the switch-off list for broken models), `dbt build`. Schedule `0 5 * * *`, `max_active_runs=1`, vault role id from Variables. Prints the current git tag (8295b3c0).
2. 2025-06-11 (90740851): waits for upstream DAGs with an external-DAGs sensor (execution delta 1 hour). 2025-06-26/27 (4153aa83, 1b602c0a): after the run, a task creates dataset (asset) events for each built model so consumers (the `ai_trigger` DAGs) start by themselves; first `email_on_failure: True` appears in the DAG on 2025-06-26 (b2b7f9c2).
3. 2025-08-08 (4caea210) "dbt retry": a re-run after a failed try uses `dbt retry` (redo only what failed) instead of a new build; 2025-10-24 clean, archive-artifacts and retry tasks; 2025-09-05 (ed743ad8) "vdpreadysignal": at the end a SOAP call tells the central scheduler that the warehouse is ready (also 2025-09-09 succesful_run task).
4. 2026-05-12 (cf48a9dc "dbt on k8s", 359 lines changed, plus `airflow_local_settings.py` and an integration test): every dbt step is a Kubernetes pod in a per-environment namespace, image `dbt-vdp:<environment>` (pulled always), secrets from a cluster secret, shared volume for `target` and `artifacts`. Schedule `25 4 * * *`; mail to the team's devops list on failure (`email_on_failure`).
5. 2026-05-27 (885e3acc/ac523bed "migratie naar dataset-driven scheduling, custom alerting"): schedule = a list of upstream datasets from a Variable `dbt_dataset_dependencies` instead of a cron; failure mail through a callback that is silent for manual runs (`email_on_non_manual_failure`, so developers testing do not flood the mailbox). Same day a hotfix chain: 0ccc90a7/223f85e9 "Fixed hosts dict call", df7dd9c2 "Updated scheduling and search window" (cron 01:25 to 01:45, sensor window 60 to 105 minutes).
6. Artifacts/state: after each run `target` is copied to an `artifacts` folder on the shared volume (`archive_artifacts_to_pvc`), results go to XCom, `list_failed_models` runs `dbt list --select result:error --state artifacts`, and the retry uses `dbt retry`. No `--defer` in the daily DAG. Retries in Airflow settings: none found (`g grep retries airflow` = 0 hits); the second try is a manual re-run.
7. Selectors: 2026-06-09 (87007058) tag `on-demand` excluded by default; 2026-07-31 (ca06255f) parameter `full_refresh` (boolean, adds the flag only for build/run); 2026-10-19 (432b08f9) scheduled runs use `build --selector daily_load` (default command used to be a plain `build`); 2026-10-23 forces the selector for scheduled and dataset-triggered runs even when the manual parameter holds another value.
8. 2026-10-02 (7c8f34c1): "compatible with Airflow 2 and 3" (new `compat.py`, parse test); 2026-10-09 49881052 "logical step for generating dataset events".

`dbt_release.py`:
- 2026-07-31 (b5a582ce, INTEL-24460): "release DAG with example jobs" (192 lines). 2026-08-04: "Hotfix/dbt release dag" (ec8ccd90, 350 lines added) turns it into the real thing: a list `RELEASE_STEPS` ("Developers: Edit this list to specify models to build during a release"), each step a dbt command in its own pod; after every step the run results and manifest are merged into an `accumulated_state` folder per run id, so the next step can defer/skip; a final step `build_remaining`. Content of that list: a full refresh of `pit_burger_db` followed by seven batch steps with `--vars {max_nr_days_batch_pit: 2}`, two further full refreshes (`satc_pit_burger_int_mon_dhb`, `satc_burger_hb_incr`), then `build` of the rest.
- 2026-11-23 (7c841200 "Add basic release DAG for dbt", 635 lines changed + 7 util modules): rewritten as "basic variant": a list `RELEASE_STATEMENTS` executed exactly as written, one after the other; no upstream build, no state merge, no final build. It pauses the daily load while it runs and unpauses afterwards whatever happens (through the Airflow API in Airflow 3), a failing step stops the rest, `dry_run` logs the plan without running. On failure a mail callback fires.
- 2026-11-26 (5dc0caec "Release DAG steps 2026_DROP26"): the first and only commit with this subject. It replaces the examples (`build_some_models`, `cleanup_user_models`) by: `perform_maintenance` (`run-operation perform_database_maintenance`, 1 thread), `daily_build` (`--selector daily_load`, 3 threads), `on_demand` (`build -s migr_stratmon_int_mon_burger_beroep_mhb+2`) and `full_refresh` (`build -s fpsm_bit_traject+ fpsm_ftr_ibo+ fagd_ftr_vacature_indicator+ --full-refresh`). So the list is edited by hand, per release, in the DAG file, and mixes one-off macro calls, on-demand models and full refreshes. The brief expected recurring commits of this kind: `g log --all --grep='Release DAG steps'` returns exactly 1 commit (DROP26 only). Earlier drops (12 to 25) have no such commit, and no release branch touched `dbt_release.py` before DROP26 (inferred: DROP18..25 simply carried whatever version `dev` had).

## 4. DAG fixes on release branches

Computed with `g log release/2026_DROP<nn> --not dev -- airflow` for each of the 14 release branches (DROP12..DROP26, DROP15 missing). Only 5 of 14 had any airflow commit that is not on dev; 9 had none.

| Drop (tag .0 date) | Date | Hash | What broke | Fix |
|---|---|---|---|---|
| 13 (2026-05-28) | 2026-05-27 | ac523bed, 223f85e9, df7dd9c2 | the dataset-driven refactor shipped on the release branch the day before the tag; DAG looked up the environment through a hosts dictionary and fell back to LDV | `DBT_DEPLOY_TARGET` now from the environment variable (3 places); schedule 01:25 to 01:45 and sensor delta 60 to 105 minutes. Same changes exist on dev (cherry-picks: different hashes, same subjects). |
| 17 (2026-07-23) | 2026-07-17 | 84844d88 | list of switched-off models was passed with the wrong separator/XCom handling, disabled models still ran | exclude list read from the Variable and joined with spaces; 102 lines. Also on dev as 38a77572 the same day. |
| 23 (2026-10-15) | 2026-10-19, 10-23, plus 10-15 revert | 025c1690, fd5cae50, 52fa811f | after the "split off non critical tests" feature (INTEL-26835, merged 10-13, reverted 10-15) the daily DAG still ran a plain `build` | default command `build --selector daily_load` (10-19); scheduled/dataset-triggered runs always use the selector (10-23). Both after the .0 tag (patch tag .1 on 10-20). |
| 24 (2026-10-29) | 2026-11-03 | 82d52450 | helper module moved/renamed for the Airflow 2 and 3 compatibility ("Rename compat to dbt_compat and fix imports"), imports broke | file move plus imports (also 4302a71d on another branch). Carries the DROP23 fixes too. |
| 26 (2026-11-26) | 2026-11-26 | d25bea4d | "Fixed airflow parsing issue": DAG files did not parse in the target Airflow (Asset class versus Dataset, Variable lookup API) | compat layer extended (`get_variable`, Dataset in place of Asset), 5 files, +88/-27. Same day as the release tag. |

Pattern: DAG changes ride along with data releases; 4 of the 5 fixes are the same change on dev and on the release branch (cherry-pick), so the DAG is fixed twice. The release branch is where DAG problems first hurt (a parse error blocks all runs of that environment, inferred).

## 5. Version numbers

Three schemes (counts via `g tag`):

| Scheme | Tags | From .. to | Created by |
|---|---|---|---|
| `v1.x.y` | 459 (v1.0.1..v1.0.96 = 97 patch-only; then 204 tags with patch > 0 in total) | 2025-05-10 .. 2026-01-14 | CI job on GitLab: `create_release` (2025-05), `tag_dev` on every merge into dev. Feature merge = minor+1, anything else = patch+1 (since 2025-07-27). Pushed with the CI job token as "GitLab CI". |
| `1.x.0` | 92, patch never used | 2026-01-23 .. 2026-06-02 (1.254.0..1.346.0) | new pipeline (Jenkins shared library, inferred; no `v`, every merge bumps minor; 23 of them point at "Edit dev.Dockerfile" commits) |
| `2.x.0` | 539 | 2026-06-03 .. 2026-11-26 (2.1.0..2.539.0) | same mechanism; major version bump with minor reset |
| release tags | 35 | `2025Q4_C5.0` (2025-12-11), `2026_DROP03.0` (2026-01-08) .. `2026_DROP26.0` (2026-11-26) | `tag_main`: merge of `release/<name>` into main gets `<name>.0`; each later push to main gets `.1`, `.2`, `.3` |

- Tags are lightweight (1,116 commit-type, 9 annotated: v1.105.1/.2 and v1.144.1..6 and `2025Q4_C5.0`, which are manual patch tags on 2025-10-16 .. 2025-11-28). No tagger name anywhere (`%(taggername)` empty); tag subjects are the merge subject of the commit they point at. 1,090 numbered tags point at "Merge branch feature/... into dev/main" commits (about 1 per merged ticket).
- Why the scheme changed (history shows the when, not the why): `v1.0.x` -> `v1.minor.patch` 2025-07-27 (19645c05: semver-like). 2026-01-14 (77657fb5 "Merge remote-tracking branch 'source_fork/main'", tag v1.253.1) is a repository hand-over; 9 days later (2026-01-23) the first tag without "v", 1.254.0, points at "Hotfix/2026 drop04" (37b5fadb) and the GitLab CI file is removed the same day (1f3b8f5f). So tag creation moved with the pipeline from GitLab CI to Jenkins (inferred). 2026-06-02/03: v1.346.0 (2accb8bd, INTEL-24488) to 2.1.0 (bc4b2e8c, INTEL-24679): nothing in the repository changes that day (no commit touches CI, Dockerfile or Jenkinsfile); the major bump is therefore made by the external pipeline (inferred, not found). Reference to the dbt upgrade: "Feature/dbt version upgrade" merged 2026-06-22 (241b15c7), three weeks later, so not the trigger.
- Release tags point at merge commits "Merge branch 'release/<name>' into main". 34 of 35 are not reachable from any branch today (`g branch -a --contains <tag>` empty; only DROP26.0 sits on `origin/main`, DROP25.0 on the stale local main): main is rebuilt for each release (merge-base reset, then merge with `--no-ff`), so the old release merge commits survive only as tags. Per release: 18 `.0` tags; 16 patch tags (`.1`..`.3`) on 12 of the 18 drops (DROP03.1, 05.3, 12.1, 14.1/.2, 16.1, 17.1/.2/.3, 19.1, 20.1, 21.1, 23.1, 24.1/.2, 25.1).
- Numbered tags per month: 2025-05 26, 06 37, 07 35, 08 65, 09 43, 10 90, 11 62, 12 82, 2026-01 25, 02 9, 03 16, 04 10, 05 44, 06 82, 07 81, 08 80, 09 116, 10 92, 11 95 (plus DROP tags: 1 to 6 a month). Peak 116 versions in Sep 2026, close to 4 a day.
- Changelog: 2025-12-08 (9bece7c4, 2b19164e) a `changelog` job writes `CHANGELOG.md` per tag with a git-changelog tool, committing "chore(ci): Update changelog for release vX [skip ci]": 18 such commits 2025-12-08 .. 12-11 on dev (up to 694 lines changed each), reverted 2025-12-11 (21ad84be "Reverted Changelog changes"). 2025-12-16 (3b4add82, ced4ad4d): moved to a separate `changelog` branch force-pushed from dev; old file removed; 2026-02-09 b779218c "rebase instead of merge". 2026-11-12 onward: `changelog/2026_DROPnn.md` committed on the release branch (5 commits "Changelog 2026_DROP25/26": 05a4f397, 4de114ab, fc93782e, d6891a99, 95a90d96). Content is generated (machine-readable header with ticket id, summary, fix version, sections "Previously reverted, now accepted (4)", "Added (40)", "Excluded (4)" with a reason such as "2026_DROP27 is after 2026_DROP26"); not hand-written. Source: ticket fix-version per drop (inferred from the header).

## 6. Branching in practice

Merges and direct commits on the first-parent line of `dev` (`g log --first-parent dev --format='%ad|%p'`, merge = 2 parents). `origin/main` has the identical line plus the release merges: its numbers are the same except 2026Q4 (134 merges vs 190, because main was reset at DROP26).

| Quarter | Merges into dev/main line | Direct non-merge commits on the line |
|---|---|---|
| 2025Q1 | 1 | 17 |
| 2025Q2 | 54 | 24 |
| 2025Q3 | 114 | 38 |
| 2025Q4 | 189 | 68 |
| 2026Q1 | 72 | 76 |
| 2026Q2 | 127 | 38 |
| 2026Q3 | 276 | 2 |
| 2026Q4 (to 11-26) | 190 | 2 |

- Until 2025-11-30 the integration branch was `main` (subject "into 'main'": 16 in May, 36 in Jun, 25 Jul, 48 Aug, 40 Sep, 63 Oct, 44 Nov 2025). First merge "into 'dev'": 2025-12-04 (ef39ad8c, same day as the "Git Flow" CI rework). Dec 2025: 60 into dev, 13 into main.
- Release-branch model: earliest names `release/2025Q4_C3` (47 merge commits 2025-10-15 .. 11-28, a six-week branch), `2025Q4_C1` (10-21), `C5` (tag 2025-12-11), then `2026_DROP03` (2026-01-08), `DROP04` (01-22, no `.0` tag; its tag is 1.254.0), `DROP05` (tag 02-13), `DROP11` (04-22), then DROP12 .. DROP26 (14 branches that still exist, DROP15 missing). Before October 2025 releases were simply the merges into main with a tag on every merge (v1.x).
- Release rhythm: 18 `.0` tags; gaps in days 28, 36, 68, 27, 9, 12, 30 and then 14 eleven times in a row. Median gap over all 17 gaps = 14 days; since June 2026 every gap is 14 days (every second Thursday, DROP16 on 07-09 to DROP26 on 11-26). The numbers DROP04, 06..10, 15 have no tag.
- Reverts: release branches carry revert commits that are not on dev (count of "Revert" subjects): DROP17 19, DROP19 17, DROP20 9, DROP22 9, DROP25 6, DROP23 and DROP24 5 each, DROP26 4, others 0 to 2 (unready features are removed from the release instead of the release being postponed; cross-check with analyst for models).
- Direct commits on the line dropped from 76 (2026Q1) to 2 per quarter from 2026Q3: web-editor and WIP commits (like the 23 "Edit dev.Dockerfile") stopped landing straight on dev (inferred: merge requests enforced).
- `staging-<user>` branches: 8 on origin, tips 2026-04-28 .. 2026-05-08, all 0 or 1 commit ahead of dev, 1,266..1,341 commits behind. They are per-developer copies made at the repository migration (one carries "WIP: migration commit", 2026-04-28); `dev-swf` (tip 2026-05-08, 0 ahead) is the dev line of the old dbt-swf repository; on 2026-05-15 the project was renamed dbt-swf to dbt-vdp (b8d834ab) and named back (23f1c061). Purpose inferred; no branch used after 2026-05-08.
- `feat/v2` (2026-11-05 .. 11-10, 2 commits): "dbt v2 changes" (Fusion/dbt 2 adapter prep, profiles.yml and `incrementeel_laden...`) and "Batched / chunked full-refresh voor de zwaarste modellen". `fmt`, `dbt-image-separation`, `YCL_activate_sonar`, `feature/dbt-reload-dag` are single-purpose branches with 1, 1, 6 and 7 commits not on dev.
- `feature/dbt-reload-dag`: 2026-09-15 .. 10-21, 7 commits not on dev, contains the blue/green reload DAG and `dbt Reload DAG setup.md` (2026-10-16: reload one table that does not fit in one batch; build `__new_<table>` next to the old one, test, rename old to backup, rename new, drop leftovers; batch size halves when a batch fails on memory). Its shared utilities reached dev only on 2026-11-23 in the release DAG commit.

## Stories worth telling

1. **A release list that is edited by hand and replaced after one night.** 2026-08-04 (ec8ccd90): the release DAG holds a hand-edited list of steps, among them a full rebuild of one big table in seven small batches so the database does not run out of memory. On 2026-11-23/26 (7c841200, 5dc0caec) it is rewritten as "run the listed statements exactly as written", pauses the daily load while it runs and unpauses it whatever happens; the first list is a maintenance step, the normal daily build, an on-demand model and three full refreshes.
   Workshop angle: rebuild without downtime; a release run is a short, reviewed list of steps, with a dry run before.
2. **Version number per merge, from 1.0 to 2.539.** From May 2025 every merged ticket got a new number (v1.0.1 on 2025-05-10; 2.539.0 on 2026-11-26): roughly 1,090 numbers in 18 months, up to 116 in September 2026. Any environment can be rolled back to "the build with number N", and the number tells which ticket was merged last.
   Workshop angle: numbered versions and rollback.
3. **The scheme changed three times and nothing in the repository says why.** `v1.253.1` (2026-01-14), then `1.254.0` without the v (2026-01-23, the day the old pipeline file was deleted), then `2.1.0` (2026-06-03): the tag creation moved to a pipeline whose code is outside the repository. Consequence: the history shows when the labels changed, not who or what decides.
   Workshop angle: numbered versions; keep pipeline definitions in the same place as the code.
4. **Two safeguards the team tried to make automatic and had to switch off.** The check that builds only the changed models with zero rows (`--empty`) was removed on 2025-11-28 for an adapter bug, re-added on 2025-12-11 and removed again on 2025-12-16 for "consistent bug" (5cf3460f, d6410a99). Workshop angle: automatic checks must be trusted to be useful; keep a fallback and know which step is off.
5. **The rule that did not catch what it should catch.** On 2026-08-19 (7ec28296) a house rule forbade two SingleStore-unfriendly functions (`to_char`, `str_to_date`) in models; on 2026-11-24 (0af07434) the rule itself was repaired because its pattern missed calls, and the same commit removes the last use. The rules run only on the developer machine, never in the central pipeline (0 hits for bouncer or sqlfmt in any CI file).
   Workshop angle: house rules / formatter, and run the same rules in the shared pipeline so a bypassed local hook is still caught.
6. **One formatter run changed 921 files.** 2025-12-19 (5d255919): the automatic SQL formatter was enabled for the whole project in one commit (+18.9k/-16.3k lines), again on 2026-09-09 for yml files (213 files). Before it, 7 commits in November 2025 were just called "formatting".
   Workshop angle: house rules / formatter; do the big reformat once, in one dedicated change, so later reviews show only real changes.
7. **Failure mail that does not bother the developers.** The daily DAG mailed the devops list on failure from June 2025. On 2026-05-27 (885e3acc) the mail was moved to a callback that stays silent for manual runs, so people testing do not flood the mailbox; on the same day the DAG stopped using a clock and started when its input data was ready.
   Workshop angle: failure mail; start on "data is ready", not on a clock.
8. **Retry only the broken part.** 2025-08-08 (4caea210): a second attempt of the daily run uses `dbt retry`, so models that already succeeded are not rebuilt; a list of failed models is produced for the next step (`list_failed_models`, 2026-05). There is no automatic retry in Airflow; a person triggers the re-run.
   Workshop angle: redo only what failed.
9. **A release bug found after the version was already out.** DROP23 (tag 2026-10-15): the daily DAG still ran a plain `build` instead of the "daily" selector; fixes on 2026-10-19 and 10-23 (025c1690, fd5cae50), after the .0 tag and the .1 patch tag. DROP26 (2026-11-26): the DAG files did not parse in the target Airflow (d25bea4d), fixed the same day.
   Workshop angle: rehearse before merge; parse and dry-run the DAG in a copy of the environment (a DAG parse test exists since 2026-10-02 but runs outside the central pipeline).
10. **Main is rebuilt for every release.** 34 of 35 release tags point at merge commits that no branch contains any more; main is reset to the integration point and the release merged again. The numbered tag, not the branch, is the only durable record. Also: releases left behind a list of "Revert" commits on the release branch (19 on DROP17, 17 on DROP19) when a feature of one team was not ready.
    Workshop angle: one team not ready at release time; numbered versions and rollback.
11. **A password in the history, noticed 17 months later.** A test password for a local database container went into the repository on 2025-05-10 (3b066a19) and was explicitly whitelisted in the secret scanner on 2026-10-02 (4c21fdb0), together with a rule that ignores the warehouse's hash keys that look like secrets. Real passwords arrive another way: each developer types theirs once into the dev container, which stores it in a local, untracked file, and builds into a schema with their own name.
    Workshop angle: own workbench per developer; automatic checks (secret scan).
12. **Thirteen "Edit dev.Dockerfile" commits in one day.** 2026-03-10: the image recipe was edited and rebuilt through the web editor 13 times (39 times in total over the year), each commit producing a new version number (23 numbered tags point at such commits). It shows what a missing local rehearsal of an image build costs.
    Workshop angle: rehearse before merge; own workbench per developer.
