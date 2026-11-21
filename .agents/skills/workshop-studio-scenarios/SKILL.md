---
name: workshop-studio-scenarios
description: >-
  How to add or change a scenario of the Workshop Studio (infrastructure/workshop-studio): choosing a scenario that
  fits (inside dbt or about the tooling around it: CI, releases, access, documentation, the developer toolbox), the
  four-part scenario file (also as a long "flow" with status labels and the closing page), the content rules for a non-technical audience, the demo building blocks (actions,
  evidence, inputs, honest outcomes, clean-up), patches, adding a backend action or evidence type, and the loop of
  validating, running live, recording, capturing and persona-reviewing. Use it whenever a scenario, demo step,
  scenario text, glossary term, patch or studio action is added, changed, debugged or reviewed, or when someone asks
  what the studio can demonstrate.
---

# Adding a scenario to the Workshop Studio

The studio (`infrastructure/workshop-studio/`, http://localhost:8085) presents short scenarios to people with **no data
background**. Its own README documents the engine, the data feeder and every option of the file format; read the
section "Add a scenario" there for field-level detail. This skill is the working method and the rules that are not
obvious from the code.

## 1. What a scenario is

One idea, told in four parts, in three to four minutes:

| Part | Field | Answers |
|---|---|---|
| 1 What goes wrong | `problem` | What happens **without** the idea, and what it costs whom |
| 2 The idea | `idea` | How it is prevented, in three sentences and one picture |
| 3 The code | `code` | The few real lines that make it work |
| 4 Live demo | `demo` | Proof on the real stack, in at most three steps |

Hard requirements:

- **Independent.** It starts from the home page, after any other scenario, in any order. The engine cleans up what
  the previous demo touched and checks the baseline before "Getting ready". A dependency on another scenario is only
  a `related` link with one plain sentence; the scenario itself stays understandable alone.
- **Short scope.** If the demo needs more than three steps or a step longer than about 30 seconds, the scenario is
  two scenarios, or the slow part belongs in `demo.setup` (getting ready, at most about a minute). The exception is a
  **flow** (`kind: flow`): one long walk through several stages of one route (3 to 8 demo steps, 1 to 8 code blocks, up to
  30 minutes), where every stage says whether it is in place today or a next step. Use a flow only when the stages belong
  together; an ordinary scenario stays short.
- **The demo proves the promise.** Every claim in the text must be visible in the evidence on screen. If the stack
  cannot show it, change the claim, not the wording around it.

### Choosing one

A good candidate has (a) a harm the audience recognises without explanation (a wrong number in a meeting, a late
report, a colleague blocked), (b) one mechanism in the stack that prevents it, and (c) evidence that changes visibly
when that mechanism is switched on or used.

Scenarios need not be about dbt models. The stack offers at least these subjects:

| Area | Where it lives | Existing scenarios |
|---|---|---|
| One subject from start to finish (group `flows`, kind `flow`) | several mechanisms of one route, every stage marked "in place today" or "next step" | `rehearsing-a-change`, `developers-toolbox`, `from-change-to-package`, `guards-inside-every-run`, `the-night-steered-from-airflow` |
| Feeds that change, arrive late or broken | feeder conditions, source freshness, Airflow variables | `switchover-date`, `late-delivery`, `skip-one-table`, `strict-where-it-matters` |
| Working on code safely | developer image, per-developer prefixes, pre-commit, CI verify | `same-toolbox`, `own-workbench`, `house-rules`, `checked-before-merge`, `only-what-changed` |
| Releasing | tags, packaged versions, release branch, manual production job, docs sites, drift audit, database roles | `numbered-versions`, `back-in-seconds`, `docs-match-version`, `docs-front-page`, `one-team-not-ready`, `production-waits`, `no-secret-fixes`, `right-keys-only` |
| The nightly run | failure mail, retry from failure, blue/green reload, run logbook | `one-clear-mail`, `redo-only-what-failed`, `rebuild-without-downtime`, `every-run-on-record` |

Before writing, check that the mechanism really exists and behaves as you think: read the CI file, the DAG or the
macro, and run the thing once by hand through the studio API. Several first drafts claimed things the stack did not
do (a notification that was never sent, three developers where the demo had two).

## 2. Files

| What | Where |
|---|---|
| The scenario | `scenarios/<id>.yaml` (file name = id, lower case with dashes) |
| Its place on the home page | `scenarios/_groups.yaml`: add the id to one group's ordered `scenarios` list |
| Words that need explaining | `scenarios/_glossary.yaml`, used as `[[term]]` in body text |
| The closing page (what is in place today, what comes next) | `scenarios/_roadmap.yaml` (optional), route `#/roadmap` in the frontend |
| Code a demo commits to GitLab | `patches/<name>/_patch.yaml` (`title`, `message`, `ops` with `path` / `find` / `replace`, or full files) |
| What the demo showed last time | `recordings/<id>.json`, written by every successful live step |
| Actions, evidence, cleaners | `backend/studio/actions/*.py`, `backend/studio/evidence.py`, contract in `actions/base.py` |
| Plain words for technical names | `backend/studio/names.py` |

`scenarios/` and `patches/` are mounted read-only into the container and read at start-up: after an edit, restart
with `make up S=workshop-studio`. A file that does not validate stops the app from starting.

## 3. The file

Copy the closest existing scenario. Skeleton; its comments are the house rules (section 4), which for some fields are
tighter than the schema (`backend/studio/models.py`, see the list below the skeleton):

```yaml
id: only-what-changed            # = file name
kind: scenario                   # optional: scenario (default) | flow (a long scenario, see section 1)
group: changes                   # a group id from _groups.yaml
title: ...                       # <= 60 characters, no [[term]]
promise: ...                     # <= 110 characters: the benefit in one sentence
minutes: 3                       # scenario: at most 10, flow: at most 30
feature: ...                     # optional, with status: the plain name (<= 50 characters) of the one thing it is about
status: today                    # optional, with feature: today (in place today) | next (a next step)
why: ...                         # optional, only with feature + status: what goes wrong without it, <= 140 characters (closing page)
tech: [..., ...]                 # optional, for the presenter: 1 to 8 keywords (<= 40 characters each), the real names behind it
environment: dev                 # dev | val | prd: where the demo runs unless an action says otherwise
related:
  - {id: checked-before-merge, why: "One plain sentence, <= 160 characters."}

problem:
  headline: ...                  # the harm, <= 12 words
  story: |                       # 40 to 90 words, a concrete little story
  cost: ...                      # who loses what, and how much
  visual: {type: timeline | flow | compare, ...}

idea:
  headline: ...                  # a plain sentence, <= 12 words
  points: [..., ..., ...]        # exactly three full sentences
  visual: {type: flow, nodes: [...], links: [[a, b], ...]}   # exactly one node with state: fix

code:                            # one to three blocks (a flow: one to eight)
  - title: ...                   # plain words
    status: today                # optional badge: today | next
    file: models/.../x.sql       # in the dbt project, or one of the two other roots below
    file_label: ...              # plain words for what the file is
    from: "text of the first line"
    to: "text of the last line"
    max_lines: 12
    notes:
      - {line: "text inside the block", says: "One sentence, <= 15 plain words."}

demo:
  intro: ...                     # what will be shown; no instructions to the presenter, no URLs
  setup: [ {action: ...}, ... ]  # "Getting ready": slow preparation goes here
  steps:
    - id: first
      title: ...
      feature: ...               # optional, with status; in a flow every step has both, except the last one
      status: today              # today | next
      why: ...                   # optional, only with feature + status: what goes wrong without it, <= 140 characters
      tech: ...                  # optional, for the presenter: one line (<= 60 characters) with the real names behind this step
      say: ...                   # at most two short sentences, no duration (the app shows "Usually about N s")
      button: ...                # says what will happen
      inputs: []                 # day | number | choice | toggle, each with label, default and help
      do: [ {action: ...}, ... ]
      show: [ {type: ..., title: ..., caption: ...}, ... ]
      outcome: ...               # a sentence, or {check, good_when, good, not_quite}
```

Where the schema is wider than the house rule: a demo takes 2 to 4 steps (house: at most three; a flow 3 to 8); `max_lines` is 1 to 80
(house: 12); a block takes 1 to 4 notes of up to 200 characters (house: 2 to 4 notes of at most 15 words); a headline
may be up to 90 characters (house: 12 words); `say` up to 300 characters (house: two short sentences); a flow may have
at most one `fix` node (house: exactly one). `tech` (a scenario: 1 to 8 keywords of 1 to 40 characters; a step: one line of 1 to 60 characters) is checked in plain
sentences too. Fill it from what the scenario really uses (its code blocks, actions, patches and the project files): the
exact names of macros, flags, selectors, jobs, Variables, DAGs and files, and nothing the code does not confirm.
Enforced as written: title, promise, the `related` sentence, the story's
40 to 90 words, exactly three points (each up to 160 characters), one to three code blocks (flow: one to eight), flow
label length, and for `feature` / `status` / `kind` the rules above (the pairing, `feature` up to 50 characters, a flow's
step limits; `--check` says each violation in a sentence).

The closing overview (the last page of the studio, `#/roadmap`) is `scenarios/_roadmap.yaml`: `title`, `intro`,
optional `closing`, and `extras: [{area, feature, status, note, why}]` for rows without a demo (an extra may always have
`why`; a step or scenario only with `feature` and `status`). The page shows a row's `why` as "Without it". Its rows come
from the `feature` and `status` of flow steps and ordinary scenarios, in the order of `_groups.yaml`; the extras are
grouped by `area`.
Write `feature` as a plain noun phrase ("Automatic data checks", never a tool name) and mark honestly: `today` only
when the demo itself shows it working.

A code block's `file` names one of three roots (`_block_path` in `backend/studio/catalog.py`); anything else fails
validation:

| Root | `file:` | Real example |
|---|---|---|
| The dbt project | a path inside it | `models/edw_dv_bdv/dummy_reload_hkey_mod.sql`, `.gitlab-ci.yml` |
| Database setup files | `../infrastructure/singlestore/<name>` (also `.sql.tmpl`) | `../infrastructure/singlestore/init_env.sql.tmpl` |
| Developer image recipe | `dev-image/<name>` | `dev-image/Dockerfile` |

The last two take a plain file name only (no sub-folder, no other `..`, nothing hidden) and have no GitLab link.

Visuals: `timeline` (5 to 8 `days` with `label`, `state`, `note`, optional `marker: {after, label}`), `flow` (3 to 6
`nodes` with `id`, `label` <= 30 characters, `state` ok/bad/warn/idle/fix, `note`; every box needs an arrow in and
out except the first and last; alternatives are a fork that joins again), `compare` (`left` / `right` with `title`,
`state`, `items`). Templating in any string: `{{ env }}`, `{{ db('im_dimensional') }}`, `{{ inputs.<id> }}`,
`{{ day(-4) }}` (today plus n days), `{{ value }}` inside an outcome.

## 4. Content rules

These came out of two rounds of review by people without a data background. They are not style preferences: each
one fixed something a reviewer could not follow.

1. **Only claim what the screen shows.** Every number, weekday, name and count in a sentence is on a card next to it
   and matches it. Never write a number the system can change (row counts, order counts): use the object `outcome`
   with `{{ value }}` or point at the card.
2. **Honest outcomes.** Where an input can lead to a result that does not match the plain sentence, use
   `outcome: {check: "<one-number read-only SQL>", good_when: zero|positive, good: ..., not_quite: ...}`.
3. **One word per thing**, the everyday one, across title, text, captions and legends: feed, table, report, check,
   rehearsal, nightly load, version, release, robot, proposal, package, toolbox. Environments are written
   development, validation, production (never dev, val, prd).
4. **No technical identifiers in visible text**: no tool, branch, job, file, account or database names. The backend
   moves them to "Technical details"; scenario text must not bring them back. The one exception is the field `tech`
   (scenario keywords and a step's line): it is for the presenter, drawn small and monospace, and can be switched off with
   "Technical names" in the top bar. It must never carry an explanation the audience needs; the plain text has to work
   without it. Words that cannot be avoided go into
   the glossary and appear as `[[term]]`, never in a title, promise or headline.
5. **Part 1** names the harm concretely: who, what they lose, how much. **Part 2** has exactly three sentences that
   do not repeat the headline, and a flow with exactly one `fix` box.
6. **Code blocks** show real instructions (not comment prose), at most 12 lines, starting and ending at a statement
   boundary, with 2 to 4 notes of at most 15 plain words. Never say the code is simple.
7. **Step text** is for the audience: no "Say:", no stage directions, no durations, no URLs. Captions tell the
   viewer where to look ("Look at the Development row").
8. **Counts in sentences use the words of the list next to them** ("1 failed, 2 not run", not "three failed").
9. **Status labels are promises.** A stage or block marked "In place today" must be what the demo really shows; what is
   only planned is "Next step". The audience reads the two badges as "works now" and "does not work yet".

## 5. Demo building blocks

Actions (`do:` and `setup:` items; each validates its parameters at load time):

| Action | Use |
|---|---|
| `feed.condition`, `feed.deliver` | make a feed late, paused, dirty, duplicated, or in another format; replay the last n days |
| `airflow.set_variable`, `airflow.delete_variable`, `airflow.run_dag`, `airflow.retry_failed` | operator settings and runs (`expect: success \| failed`) |
| `airflow.wait_for_dag` | wait until the scheduler has read a setting that is read when a job's file is parsed (`dbt_dataset_dependencies`, `dbt_emit_asset_events`): `schedule: assets\|clock`, `has_tasks`, `lacks_tasks` (up to about 30 s; put it in `demo.setup`) |
| `airflow.unpause_dag` | switch a job on (the notice and the on-demand job are created paused); the cleaner `dag` pauses it again |
| `airflow.report_feeds` | `feeds: [orders, orders, carrier]` report in one by one (an asset event each, posted through the scheduler's container); `data.reasons` say what the nightly load still waits for |
| `airflow.wait_for_run` | wait for a run that started by itself (`after: <id of unpause_dag or report_feeds>`), say who started it and after how long |
| `airflow.announcements` | what the newest nightly run announced: how many tables and whether the named `tables` were among them (read-only) |
| `dbt.run` | a dbt command as the environment's robot, the CI robot or a developer; `explain: true` adds a line "Built 30 tables and ran 36 checks." for a `result` card |
| `sql.exec` | one statement as a named account; needs `undo` when it changes something; `expect: refused` for access demos |
| `gitlab.branch`, `gitlab.commit` (a patch), `gitlab.merge_request`, `gitlab.merge`, `gitlab.wait_pipeline`, `gitlab.play_job` | proposals, rehearsals, numbered versions, the manual production step. `merge_request` takes `touches: [models:<env>:<model>, ci:copies]` for what the rehearsal may change outside its private copy (say it on the proposal: the harm happens when the pipeline runs, also if the step is never played). `wait_pipeline` returns `rehearsed` (the round with data and checks) and, once the first round has finished, `structure` (the empty round); `report_rehearsed: true` puts both in the summary |
| `git.revert_on_release`, `git.promote_release` | the release flow |
| `deploy.set_version` | point an environment at another packaged version |
| `precommit.run` | the formatter and rule checker on a patch |
| `docs.read_description` | what the published documentation of each environment says |
| `docs.read_front_page` | the generated front page of each environment's documentation: the version it names, when, and the counts of tables, checks, feeds and teams; `expect: running` fails when it names another version than the one that runs |
| `image.versions` | tool versions inside the developer toolbox and the production package |
| `toolbox.run` | what a developer (`as: developer:pu_devN`) or the development robot (`as: service`) does in the toolbox, in a throwaway container: `commands` with `expect: success \| refused \| failed`, `say` lines with `{facts}`, `must_contain`, `changes`; `auth: password \| file \| vault` for how the password arrives; `use_repository` for project files that are not packaged yet. Development only; the cleaner `toolbox` drops a developer's own copies, a robot run needs a `sql.exec` with `undo` registered first |
| `storage.versions`, `storage.environment_copy`, `storage.description`, `storage.catalogue_names`, `package.open` | read-only views of what a numbered version leaves behind: the numbers and their changes (`show: numbers \| codes \| stored`), the environment's stored copy and its note, a report's stored description, table names in the stored catalogue file (`compare_with: developer:pu_devN`), the sealed packages (`versions: [baseline, latest]`) |
| `mail.clear`, `wait` | housekeeping |

Evidence (`show:` items): `bars`, `number` (with `compare_to`), `table`, `tasks`, `pipeline`, `run_history`,
`freshness`, `versions`, `result` (plain lines, also from an action's `reasons` via `from_action`), `mail`, `stack`
(up, starting or down per service of the stack, the data of the home page's status row; no scenario uses it and the
frontend draws no card for it). Any item
takes `title` and `caption`; `before_of: <step id>` shows the same item of an earlier step as the "Before" picture.
An action may take `id: <name>`: its result is kept under that name, so a step that runs one action several times
(`toolbox.run` as two developers, or with and without a guard) shows each run with `result` and
`from_action: <name>`; `airflow.wait_for_run` finds its `after:` action the same way.

Clean-up is automatic: each action reports what it touched (`variable:dev:...`, `dag:dev:...`, `feed:...`, `git`,
`version:prd`, `models:dev` or `models:dev:<table>`, `developer:...`, `toolbox:dev:<user>`, `ci` or `ci:copies`, `sql`,
`mail`, `landing:dev`), and the matching cleaner undoes it before the next
scenario. A scenario never contains clean-up steps. Only `sql.exec` needs its own `undo`.

Prefer reusing these. New backend code is justified when the proof needs data the stack has but no action or
evidence exposes; it is not justified to decorate.

## 6. When new backend code is needed

- An **action** is a function registered with `@action("area.name", ParamsModel, label)` in `backend/studio/actions/`.
  It takes `(ctx, params)`, waits for the real end state with `ctx.wait_until` (never a fixed sleep), honours
  `ctx.check_cancel()`, and returns `Result(summary, details, touched, data)`. `summary` is one plain sentence
  (run it through the `names.py` helpers); everything technical goes to `details`. Whatever it changes must be in
  `touched`, also when it fails (`StepFailed(..., touched=...)`), and a registered `@cleaner(prefix)` must undo it
  idempotently.
- Return lines for the audience as `data["reasons"]` and show them with a `result` item: that needs no frontend work.
  A new **evidence type** or a new field that must be drawn means frontend work in `frontend/src/components/` and
  `frontend/src/api/types.ts`, with every new field optional (old recordings lack it).
- Parameter models forbid unknown keys, so a scenario cannot use an option before the backend has it: the app would
  refuse to start.
- Add unit tests in `backend/tests/` (fakes in `conftest.py`); the studio README, section "Tests", has the command (a
  throwaway container, run from `infrastructure/workshop-studio/`, not from the repository root).

## 7. The loop

Run everything from the repository root; the stack must be up (`make up`).

1. **Validate**: `docker exec workshop-studio python -m studio.catalog --check` (reads the files on disk; prints
   every problem at once).
2. **Restart** to load scenario, patch or backend changes: `make up S=workshop-studio`, then wait until
   `baseline.status` is `ready` in `curl -s -H 'Host: localhost:8085' http://127.0.0.1:8085/api/state`.
3. **Run it live** with the bundled runner (host `python3`, standard library only):
   `python3 .agents/skills/workshop-studio-scenarios/scripts/run_scenario.py <id>`.
   It prints getting-ready time and, per step, seconds, status, summary, outcome and whether each evidence item has
   data. Use `--set <step>.<input>=<value>` to try the wrong choices too: the outcome must stay true for them.
4. **Read the real evidence** (the recording, or `GET /api/state` after a step) and make every sentence match it.
5. **Prove independence**: run it between scenarios of other groups, then clean up, for example
   `run_scenario.py back-in-seconds <id> one-clear-mail <id> --cleanup`. Afterwards nothing is left behind, GitLab
   holds only the baseline (tag `v1.0.0`, no demo branches, no open proposals) and every environment runs `v1.0.0`.
6. **Recording**: the live runs above wrote `recordings/<id>.json`; replay mode plays it. Re-run live after any
   change to a step's `show` list, otherwise that step counts as not recorded.
7. **Look at it**: open the scenario in the browser, or capture every screen and control without one:
   `BASE_URL=http://127.0.0.1:8085/ SCENARIOS=<id> OUT=<folder> infrastructure/workshop-studio/frontend/e2e/run.sh`
   (Docker only; switches the app to replay while it runs and back afterwards).
8. **Review as the audience** (section 9), fix, and repeat from step 1.

## 8. Pitfalls that cost time

- Right after a commit GitLab still reports the previous pipeline, and the proposal's own commit id lags too:
  `gitlab.wait_pipeline` compares with the branch's newest commit. Do not work around it with a wait.
- Two accepted proposals seconds apart make GitLab cancel the first one's numbering, so both changes end up under
  one version number. Wait for the first number (`for: "tag:^v1\\.1\\.0$"`) before the second accept.
- What the state-aware rehearsal counts as "changed" is not what intuition says: one `.sql` file gives that table
  and its dependants, a description in a `.yml` file gave nothing, a project-wide `meta` gave most of the tables and a
  project-wide post-hook all of them. Measure it with a real proposal before writing a number or a reason into the text.
- A proposal is rehearsed in two rounds: `verify_structure` (changed tables and their direct dependants, empty, no checks)
  and then `verify_feature_to_dev` (data and checks, `needs` the first). The rehearsal may build in shared tables or leave
  copies in the rehearsal database even when the step that waits for it is never played: declare that with `touches` on
  `gitlab.merge_request`.
- A job log holds several `Processed:` lines; the one after `Finished 'build'` is the build.
- The run logbook (`<env>_vdp_mgmt.dbt_run_results`) is only written by Airflow runs. Do not use it to decide
  whether an environment is built.
- "Seeded" means the 90-day baseline is there, not "the table has rows": the background feeder adds today's rows to
  an emptied table within a minute.
- Code blocks can only read the dbt project, `infrastructure/singlestore/` and `infrastructure/dbt-singlestore-dev-image/`
  (the last two by plain file name, see section 3).
- Durations vary run to run; the app shows the median of the recorded runs. Do not write them into text.
- Production is part of the practice stack, but keep demos out of it unless production is the point
  (`right-keys-only`, `production-waits`); the `version:prd` cleaner puts it back.

## 9. Review before calling it done

Checklist (each line was a real finding at least once):

- Does the demo prove the promise, or only assert it? Name the card that shows it.
- Every number in a sentence: where is it on screen? Do counts add up (built + not run = total)?
- Does it still read true with every input value, and in replay?
- Would someone who has never seen a pipeline understand each word in the title, the buttons and the captions?
- Does it run after any other scenario and leave nothing behind?
- Is each step under about 30 seconds and getting ready under about a minute?
- Every "in place today" mark (stage, block or scenario): which step or card shows it working? Every "next step": is it
  really not there? Does the closing page still read true?

For a real check, give the YAML, the recording and the captured screens to a reviewer with a persona (some
technical knowledge, no data background) who must not edit anything and must return findings as
`id | severity | scenario | field | what is wrong | REPLACE WITH: exact text`, plus a verdict per scenario and a
short KEEP list. A second reviewer with a different job (finance, customer service, product) finds different things.
Apply the findings with a separate pass, re-record, and capture again.
