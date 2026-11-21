# Workshop Studio

The presenter's app for the workshop. It walks a non-technical audience through small scenarios (four parts each:
what goes wrong, the idea, the code, a live demo; there are 26 today in five groups, see `scenarios/` and `_groups.yaml`;
the first group holds five longer "flows") and ends with a closing overview of what is in place today and what comes
next. The live demo drives the real stack: it feeds data, triggers
Airflow, commits to GitLab, changes settings and shows what happens. Every scenario can be started alone, in any
order; whatever an earlier demo left behind is cleaned up first.

## Start it

It is part of the default stack (`make up`, service `workshop-studio`) and is published on
`http://127.0.0.1:8085` only (override with `WORKSHOP_STUDIO_PORT`). It holds the Docker socket, the GitLab root
token and the database root password, so never expose it. Requests need the header `X-Studio: 1` and a local `Host`.

At startup it checks the baseline (landing data, a deployed version, built tables in dev, val and prd) and repairs
what is missing; progress is in `GET /api/state` (`baseline`). It waits first until every environment's landing tables
exist (at most five minutes). On the very first start it is up long before bootstrap has created the tables and built
`dbt-vdp:v1.0.0`, so that first check cannot succeed: `make bootstrap` therefore ends by resetting the studio (see
"After `make reset`") and waiting until its baseline is ready. Getting ready (`prepare`) does that full check
(data, version and tables of every environment; the version read goes through the Airflow CLI and takes seconds)
only after start-up or a reset, after a failed or cancelled job, for an environment where a `version:` or `variable:`
leftover was just put back, and when the last full check is older than 10 minutes. Otherwise it runs a quick check:
one read-only SQL statement per environment, at the same time (every landing table holds rows older than 30 days, so the 90-day baseline and not just today's rows; the
working and report tables exist), well
under a second; an environment that fails it gets the full check and repair. A background feeder delivers one simulated day to every
environment each `STUDIO_FEED_SECONDS` (default 60) while no job runs.

The two "A" buttons in the top bar make all text smaller or larger. There are five sizes, labelled in Flemish: Extra
klein (scale 0.45), Klein (0.55), Normaal (0.65, the start), Groot (0.75) and Extra groot (0.85). The choice is kept in
the browser (`localStorage`, key `studio-text-size-v2`; the older key `studio-text-size` belonged to a different table of
sizes and is no longer read), so set it once on the machine and screen you present from. The base size also grows with
the width of the window (`html` in `frontend/src/styles/base.css`, whose fallback is the default 0.65); the sizes are in
`frontend/src/lib/textSize.ts` and repeated in `frontend/index.html`.

Next to them sits the switch "Technical names" (on at the start; `localStorage`, key `studio-tech-names`). It shows or
hides the small technical lines that the presenter, who owns the platform, can read at a glance: the real names of macros,
flags, jobs and files behind a scenario (see `tech` under "Add a scenario"). The scenario search matches those keywords
whether or not the lines are shown.

## The data

The studio is the only thing that fills the landing tables (`<env>_landing.raw_*`). There is no simulated calendar:
"today" is the real date, `{{ day(n) }}` in a scenario means today plus n days, and no row is ever stamped after the
current time. So the dbt checks that compare with the real clock (freshness, "not in the future") mean what they say.

- **Baseline (`seed`)**: the 90 full days before today plus today up to now. It is generated per calendar day from a
  fixed seed: the same day, conditions and seed always give the same rows and ids.
- **`feed.condition`** sets how a feed behaves from now on: `paused` (nothing arrives), `late_days: k` (each day's rows
  arrive k days later), `duplicate_pct`, `dirty_pct`, `bad_rows`, `bots`, and for the carrier `format` / `format_from`
  (the payload shape is chosen per day by the day the scan arrives).
- **`feed.deliver days=n`** means "the last n days (today counts as day 1) happen again under the conditions set now":
  the rows of those feeds that arrived on or after the start of that window are deleted and the window is generated
  again. Calling it twice gives the same table. A paused feed has nothing in the window, so its newest row is at least
  n-1 days old and the feed is late; rows that would arrive after now (late feeds) are not there yet.
- **Background**: every `STUDIO_FEED_SECONDS` the rows of today whose arrival time lies between the previous round and
  now are added under the current conditions. They are the same rows a later reseed would produce for that time span.
- **Clean-up**: `landing:<env>` reseeds the baseline, `feed:<env>:<feed>` resets that feed's conditions.
- `GET /api/state` shows today's date as `feeder.simulated_day` (the frontend shows it); the newest arrival per feed is
  in the feeder status.

## Where things live

| What | Where | Notes |
|---|---|---|
| Scenarios, groups, glossary, closing overview | `scenarios/*.yaml`, `_groups.yaml`, `_glossary.yaml`, `_roadmap.yaml` | bind-mounted read-only: edit, then restart the service. `_groups.yaml` lists each group's `scenarios` in the order they are shown (home page, "Scenario N of M", previous/next); every scenario file must be listed once, under its own group. `_roadmap.yaml` is optional (see "Add a scenario", step 5) |
| Code changes a demo commits | `patches/<name>/` (`_patch.yaml` plus full files) | bind-mounted read-only |
| Recordings | `recordings/<scenario>.json` | bind-mounted read-write; written after every successful live step |
| Touched set, feeder state, git baseline | volume `workshop_studio_state` at `/data` | |
| Backend | `backend/studio/` (`actions/`, `feeder/`, `engine.py`, `catalog.py`) | tests in `backend/tests/` |
| Frontend | `frontend/` | built into the image by the Dockerfile |

Code blocks are read from the dbt project (`/workshop/dbt-singlestore-workshop`), for the database setup files from
`../infrastructure/singlestore/<file>` and for the developer image's recipe from `dev-image/<file>` (`file:
dev-image/Dockerfile`; the folder `infrastructure/dbt-singlestore-dev-image` is mounted read-only at
`/workshop/dev-image`, setting `DEV_IMAGE_DIR`). The last two take a plain file name only (no sub-folder, no `..`,
nothing hidden) and have no GitLab link. A block is shown dedented (the whitespace all its lines share is removed; the
line numbers stay the file's) and `` ``double backticks`` `` of comment and docstring lines are stripped; a scenario's
`line:` still matches the text as it is in the file.

## After `make reset`

`make reset` drops and re-creates the environment databases and rebuilds `dbt-vdp:v1.0.0` from the project in the repo,
and then runs `docker exec workshop-studio python -m studio.reset` itself (the Makefile does; it only prints a warning
when the studio is not running or a demo is busy). Without that call the studio notices by itself: when every environment has lost its data, it forgets its saved state (what demos
left behind, the saved code baseline) the next time it checks the baseline, and an environment that had to be reseeded
also gets the baseline version deployed again, even when its `dbt_image_tag` setting is still there. To do it on
purpose, or when the studio keeps talking about something that is gone, forget the saved state and start a new check:

```bash
docker exec workshop-studio python -m studio.reset
```

It forgets what demos left behind, the feeder state and the saved code baseline, then the app checks data, version and
tables of every environment again (progress in `GET /api/state`, `baseline`; about 70 seconds when everything has to be
rebuilt, a few seconds when it is intact). A baseline check that is still running (the one from a cold start) is stopped and replaced by a new one. It
refuses with a plain sentence while any other job runs (a demo step, getting ready for a scenario, clean-up); wait or
press Cancel, then run it again. `docker exec workshop-studio python -m studio.reset --wait 600` also waits until the
baseline is ready and exits 1 when it fails or the time is up (that is what bootstrap runs). No file has to be deleted by hand and
the service does not need a restart. It uses `POST /api/reset` (with the `X-Studio: 1` header).

A request to open a scenario or to clean up that arrives while the baseline check runs waits for it (at most five
minutes, `waiting` in `GET /api/state` says so) instead of being refused.

Recordings in `recordings/` stay valid as long as a step's `show` types are unchanged; run each scenario once in live
mode to refresh them.

## Add a scenario

The method (choosing a scenario, the content rules, the run / record / capture / review loop, pitfalls) and a runner
script are in the skill `.agents/skills/workshop-studio-scenarios/`. The field-level reference:

1. Copy the closest `scenarios/<id>.yaml`; the file name is the id; pick a group from `_groups.yaml` and add the id
   to that group's `scenarios` list, at the place where it should appear.
2. Fill the four parts. Use only actions and evidence types that exist (`feed.*`, `airflow.*`, `dbt.run`, `sql.exec`,
   `gitlab.*`, `git.*`, `deploy.set_version`, `precommit.run`, `mail.clear`, `wait`, `docs.read_description`,
   `docs.read_front_page`, `image.versions`, `toolbox.run`, `storage.*`, `package.open`; evidence `table`, `bars`,
   `number`, `tasks`, `pipeline`, `mail`, `versions`, `result`, `freshness`, `run_history`, `stack`). `stack` is the
   check behind the status row of the home page (`stack` in `GET /api/state`, read every 10 seconds in Live mode, empty
   in Replay): per service (code storage, database, one scheduler per environment, mailbox, data delivery) a `status`
   `up`, `starting` or `down` and a plain `detail`; a service that cannot be reached is `down`, never an error. No
   scenario uses it and the frontend has no card for it. Cleanup is derived from what the actions touched; only
   `sql.exec` needs an `undo`. The touched kinds are undone in this order: `dag` (a job that was switched on is paused
   again), `variable`, `feed`, `sql`, `mail`, `git`, `version`, `developer`, `toolbox`, `ci`, `landing`, `models`
   (`models:<env>` rebuilds every table of the environment, `models:<env>:<model>` only that one; `ci:copies` drops what a
   rehearsal left in the rehearsal database). The cleaners of `git` and `version` also drop the stored packages of numbers
   that no longer exist and put the environment's stored copy back to what the original version stored.
3. Write for people who do not know the terms; technical words go in the glossary and appear as `[[term]]`.
   Optional fields (all of them may be left out):
   - `outcome` of a step is a sentence (`{{ value }}` is shown with thousands separators when it is a whole number: 3,432), or an object that is honest about the result:
     `{check: "select count(*) ...", good_when: zero|positive, good: "...", not_quite: "... {{ value }} ..."}`. The
     engine runs the one-number `check` (read-only, as root, 5 seconds) after the actions succeed and shows `good` or
     `not_quite`. A check that cannot run leaves the step done with "its result could not be checked".
   - an input's `help` is a sentence or, for a toggle, `{on: ..., off: ...}`; a `day` input may have
     `mark: {day: "{{ day(-4) }}", label: "..."}` (a tick on the slider, label up to 40 characters).
   - `bars` and `number` return `unit` (plural noun, or null): the item's own `unit:` or, failing that, a guess from the
     tables the query reads ("parcel scans", "orders", ...).
   - every sentence the UI shows has database accounts and databases in plain words (`studio/names.py`:
     `svc_dev_dbt` is "the development robot", `prd_edw_dv_bdv` is "the working tables of production"); the technical
     sentence goes to the details.
   - a `table` may have `footer: <line>` (returned as `footer`) and `unit:` on a column (returned in `columns`); it shows the technical name of a dbt check (`source_not_null_raw_olist_raw_olist_orders_order_id`) in
     plain words ("Orders feed: the order number is never empty", `studio/names.py`); the row keeps the technical
     name in its `details`.
   - `bars` return the bars oldest first, today last: `bars: [{key, label, today}]` next to `labels`/`series`; `run_history`
     returns per run `tables_built`, `tables_total` (tables in that run), `tables_not_run`, `tables_failed`,
     `tables_left_out` (project tables minus `tables_total`; the project's size is the largest model count of development's
     `target` and `target_drift_audit` manifests, the same source as the `rehearsed` total; 0 when unknown or for a retry), `checks_passed`, `checks_failed`, `checks_warned`, `checks_not_run`, `retry` (a small run that redid what the
     run before it left undone), `state` (`good|warn|bad`) and a `summary` ("30 of 30 tables built, 21 checks passed",
     "26 of 30 tables built, 4 tables and 2 checks not run, 1 check failed", "27 of 30 tables built, 3 left out on
     purpose, 14 checks passed", "Retry: 4 tables built, 3 checks passed"); built + failed + not run = `tables_total`; the
     `pipeline` card lists jobs in the order they run and, when a verify job failed on a data check, `failures:
     [{kind: check|table, text, rows, details}]` read from the job log; `tasks` collapse bookkeeping steps into one row with a `count`, mark a skipped clean-up or failure-mail step of a run
     without any failed step `not_needed: true`, number repeated "Loading one part of the new copy" rows (`part`,
     `parts`) and say "Not run" for a step that never ran after a failed one;
     code blocks hide the literal after `IDENTIFIED BY` / `PASSWORD` / `password:` (`'••••••'`).
   - a `show` item may have `before_of: <earlier step id>`: the item at the same position and of the same type in
     that step is shown as the "Before" picture.
   - `gitlab.wait_pipeline` always returns `rehearsed: {tables, checks, tables_total, seconds, names}` in its result
     `data` when the proposal's pipeline has a finished `verify_feature_to_dev` job: `tables` and `checks` are read from
     dbt's closing line of that job (`Processed: 1 hook | 4 models | 3 tests`), `seconds` is dbt's own build time from
     the closing line of the build (`Finished 'build' ... [1.9s]`, a number, null when absent), `names` are the plain
     names of the rehearsed tables via `names.model_title` (at most 6, then one entry "and N more"), and
     `tables_total` is the number of models in the manifests of development (the larger of the last run's and the
     drift audit's; without them, the `Processed:` line after `Finished 'compile'` in the job's own log). `report_rehearsed: true` adds "It rehearsed <n> of <total> tables and <n> data checks in <s> seconds." to the
     summary; `pipeline` evidence with `rehearsed: true` carries the same object as `rehearsed` (null until the job
     has finished). Both are opt-in, so older scenarios keep their sentences. The raw log tail stays in `details`.
   - A proposal's pipeline has two jobs in stage `verify`: `verify_structure` (the empty first round: the changed
     tables and their direct dependants, no rows, no checks) and then `verify_feature_to_dev` (real data and every check on
     the changed tables and what is built on them; it `needs` the first, so a failed first round leaves it "Not run").
     When the first job has finished, `gitlab.wait_pipeline` also returns `structure: {tables, tables_total, seconds,
     names}` read from its log in the same way, and with `report_rehearsed: true` the summary says "The first round built
     <n> of <total> tables." (or, when there is no job with data, "It rehearsed <n> of <total> tables, without any data
     check."). Both are optional: on a baseline with only one job nothing changes.
   - `gitlab.merge_request` takes `touches: [...]`: what the rehearsal that starts with the proposal may change outside its
     private copy, so the clean-up puts it back: `models:<env>:<model>` (a shared table the rehearsal was allowed to build:
     the cleaner rebuilds just that table with the deployed version) and `ci:copies` (tables a rehearsal left behind in the
     rehearsal database: the cleaner drops every table there). Say it on the proposal, not on the wait, because the harm is
     done as soon as the pipeline runs, also when the step is never played.
   - `gitlab.play_job` returns `reasons` (two plain lines: "Approved by <name>", "Approved on 15 November, 18:06",
     local time without "(UTC)") for a `result` item; the job name is only in `details`.
   - `docs.read_description` reads the description of one report from the published documentation page of each
     environment (parquet files over HTTP from `workshop-dbt-docs`, so `pyarrow` is a dependency) together with the
     version the environment runs; `reasons` has two lines per environment ("Development: version v1.0.1, documentation
     page written 15 November, 18:07", then the sentence). Whether the sentence equals the one in that version's package
     is in `details` (and a third line only when it is not). `expect: same|different` fails the step when the first two
     pages do not behave that way.
   - `image.versions` opens a throwaway container of the developer toolbox and lists a fixed, named set of tools
     (`probes`: "Report builder", "Database connector", "Layout robot", "Rule checker", "Helper tool", "Pre-save
     checks"). `compare_with: toolbox` opens a second fresh toolbox and compares the two; `compare_with: production`
     compares with the package production runs; `expect_same` names probes that must match, otherwise the step fails.
     Result `data.reasons` is a list of objects: `{label, left}` for a single listing, `{label, left, right, same}` for a
     comparison (`left`/`right` are the visible texts such as "version 2.0.11", "installed" or "the same build"; `right`
     is null for a tool missing on that side); `data.left_title` / `data.right_title` name the columns ("Toolbox";
     "First toolbox" / "Second toolbox"; "Toolbox" / "Production package"). Technical names and hashes are only in
     `details`. Nothing is changed. The other actions return `reasons` as plain strings.
   - The scheduler's own switches (`airflow.*`): `wait_for_dag` (until a setting read at parse time has taken effect: `dag`, `schedule: assets|clock`, `has_tasks`, `lacks_tasks`; up to about 30 s, so it belongs in `demo.setup`), `unpause_dag` (the notice and on-demand jobs are created paused; the cleaner `dag` pauses them again, and a `run_dag` of the on-demand job does the same), `report_feeds` (`feeds: [orders, orders, carrier]`: a feed reports in by an asset event posted through the scheduler's container; `data.reasons` say what the nightly load still waits for), `wait_for_run` (`after: <id of an unpause_dag or report_feeds item>`: the first run that began after it, who started it and after how long), `announcements` (read-only: how many tables the newest nightly run announced and whether the named `tables` were among them). Clearing either of the two parse-time settings clears both and waits for the re-parse. `dbt.run` takes `explain: true` for a "Built 30 tables and ran 36 checks." line.
   - An action may take `id: <name>`: its result is then kept under that name as well, so several runs of one action in a
     step each get a card, through `from_action: <name>` of a `result` item (without `id` the newest result of the action
     name is shown). `wait_for_run` reads its `after:` the same way.
   - `toolbox.run` (development only) runs what a developer does in the developer toolbox, in a throwaway container of the
     developer image with the project copied out of the package that development runs: `as: developer:pu_dev1|pu_dev2|pu_dev3`
     or `service` (the development robot), `auth: password|file|vault` (how the password reaches the toolbox: in the
     environment, in the settings file `.devcontainer/.env` of the copy, or only two vault ids in that file; the robot stays
     on `password`), `commands: [{run, expect: success|refused|failed, say, must_contain, changes}]` (1 to 8, each its own
     shell script in the project folder; `say` holds plain lines with `{facts}` such as `{tables}` or `{errors}`; the first
     line of the action is the summary, the rest are `reasons`; `changes: true` also reports what the command changed in the
     project's `.yml` files) and `use_repository: [paths]` (project files taken from the repository instead of the package,
     for a change that is not packaged yet). A developer's own copies are dropped by the cleaner `toolbox:<env>:<user>`; the
     robot has none and builds in the shared tables, so a scenario that runs it registers the undo first (a `sql.exec` with
     `undo`). Passwords and vault ids never appear in any output.
   - The stores of a numbered version (all read only): `storage.versions` (`show: numbers|codes|stored`: the numbers and
     the changes behind them, the same changes with and without a number, or what the package registry of GitLab holds),
     `storage.environment_copy` (`show: copy|note`, `expect_match`: the environment's copy in storage and what its note says),
     `storage.description` (`version`, `model`: the description of a report as stored with a version),
     `storage.catalogue_names` (`version`, `models`, `compare_with: developer:pu_dev1|2|3`: table names in the stored
     catalogue file next to the names a developer's own compile gives) and `package.open` (`versions`, one or two of
     `baseline`, `previous`, `latest` or a number: the number printed on the sealed package, the tools in it and the
     description in its code). They return `reasons` (plain strings or `{label, left, right, same}`), and where the table has
     headings `label_title`, `left_title` and `right_title` name them.
   - `docs.read_front_page` (`envs`, `expect: running`) reads the front page of each environment's documentation
     (`scripts/docs_overview.sh` of the dbt project writes it at deploy time): the version it names, when it was written and
     the counts of tables, checks, feeds and teams, or that it is still the standard page. `expect: running` fails the step
     when a page names another version than the one that runs.
   - `tech` (the presenter's technical line, never needed to follow the story): a scenario or flow may have `tech: [..]`,
     1 to 8 keywords of 1 to 40 characters, and a step may have `tech: "..."`, one line of 1 to 60 characters, both
     independent of `feature` and `status`. They are served as `tech` in `GET /api/catalog` (scenario and `steps`) and in
     `GET /api/scenarios/<id>` (scenario and step; absent on a step without it). The frontend draws them small, muted and
     monospace, joined with " · ", under the promise on a home card, under the title on every part of a scenario, on a step
     card of the demo, and on the closing page under a row (a flow stage shows its own line, an ordinary scenario its
     keywords), unless the switch "Technical names" is off. Use the real names (`state:modified+`, `drop_nodes`,
     `dbt_dataset_dependencies`), checked against the code the scenario shows; an item the code does not confirm is left out.
   - a flow node may have `state: fix` (the safeguard; at most one per flow); a code block may have `file_label`
     (plain words, up to 60 characters, shown instead of the path).
   - **Flow (a long scenario):** `kind: flow` (default `scenario`). A scenario has 2-4 demo steps, 1-3 code blocks and
     at most 10 minutes; a flow has 3-8 steps, 1-8 code blocks and at most 30 minutes. The home card of a flow says
     "Flow · 6 stages"; with more than 4 steps the demo part gets a small map of buttons that jump to a step, and with
     more than 3 code blocks the code part gets one that jumps to a block.
   - **Status labels** ("In place today" or "Next step", shown as a badge): a step may have `feature` (1-50 characters,
     the plain name of what this stage is about) and `status: today|next`, which come together or not at all; a code
     block may have `status: today|next`; a scenario may have `feature` + `status` too (same pairing rule). In a flow
     every step needs `feature` and `status`, except the last one (the "everything switched on" stage). Step, scenario
     and roadmap extra may also have `why` (1-140 characters, one plain sentence: what goes wrong as long as the feature is
     not there; shown on the closing page as "Without it"). On a step or scenario it needs `feature` and `status`; an
     extra may always have it.
4. Check it, with the dbt project and the SQL folder mounted (`SCENARIOS_DIR`, `DBT_PROJECT_DIR` and `SQL_DIR` point
   at them): `python -m studio.catalog --check` lists every problem at once. A failing check is also a startup
   error of the app.
5. The closing overview: `scenarios/_roadmap.yaml` (optional, checked like the other `_*.yaml` files, unknown keys
   refused): `title` (up to 60 characters), `intro` (up to 300), `closing` (up to 300, optional) and `extras: [{area (up
   to 40), feature (up to 60), status: today|next, note (up to 160, optional), why (up to 140, optional)}]` for rows that
   have no demo of their own.
   `GET /api/catalog` returns it as `roadmap` (null without the file), and each catalog scenario carries `kind`,
   `feature`, `status`, `why` and `steps: [{id, title, feature, status, why}]`. The page `#/roadmap` (one closing card at the bottom of
   the home page; it works in Replay too, it is static content) lists, in the order of `_groups.yaml`, each flow with the
   `feature` and badge of its stages, each ordinary scenario that has `feature` + `status` as one row, then the extras
   grouped by `area`, under two counts (in place today, next steps). Every row links to its scenario. Without any status
   the page says so in one sentence.

## Throwaway containers

Every container the studio starts for one step (`studio-<8 hex>`: a dbt run, the pre-commit run, the toolbox listing and the toolbox runs,
copying the project out of a package, reading a file from a package, deploying a version) carries the label
`workshop.studio.throwaway=1` and is removed when its step ends, also when the step fails or is cancelled. A restart
in the middle of a step cannot do that, so at start-up the studio removes every container with that name pattern AND
label (and nothing else: `workshop-studio` and the personal `singlestore` are never touched; the feeder is no
container but a thread named `studio-feeder` inside the app).

## Replay

Mode `replay` (`POST /api/mode {"mode": "replay"}`, or the switch on the home page) plays recorded steps back, about
ten times faster, without touching the stack. A step without a recording says so. To record, run the steps once in
live mode; each successful step overwrites its entry in `recordings/<id>.json`. A recording keeps the type and
title of every evidence item, when it was made, and the last durations (their median is the "usually takes" shown
in the UI, as is the time of getting ready). A step whose `show` types no longer match its recording counts as not
recorded.

## Tests

Backend unit tests, in a throwaway container, from this folder (`infrastructure/workshop-studio/`):

```bash
docker run --rm -v "$PWD/backend:/b" -w /b python:3.13-slim sh -c "pip install -q -r requirements.txt && python -m pytest -q"
```
