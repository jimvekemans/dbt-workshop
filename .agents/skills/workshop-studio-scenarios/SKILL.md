---
name: workshop-studio-scenarios
description: >-
  How to add or change a scenario of the Workshop Studio (infrastructure/workshop-studio): choosing a scenario that
  fits (inside dbt or about the tooling around it: CI, releases, access, documentation, the developer toolbox), the
  four-part scenario file, the content rules for a non-technical audience, the demo building blocks (actions,
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
  two scenarios, or the slow part belongs in `demo.setup` (getting ready, at most about a minute).
- **The demo proves the promise.** Every claim in the text must be visible in the evidence on screen. If the stack
  cannot show it, change the claim, not the wording around it.

### Choosing one

A good candidate has (a) a harm the audience recognises without explanation (a wrong number in a meeting, a late
report, a colleague blocked), (b) one mechanism in the stack that prevents it, and (c) evidence that changes visibly
when that mechanism is switched on or used.

Scenarios need not be about dbt models. The stack offers at least these subjects:

| Area | Where it lives | Existing scenarios |
|---|---|---|
| Feeds that change, arrive late or broken | feeder conditions, source freshness, Airflow variables | `switchover-date`, `late-delivery`, `skip-one-table`, `strict-where-it-matters` |
| Working on code safely | developer image, per-developer prefixes, pre-commit, CI verify | `same-toolbox`, `own-workbench`, `house-rules`, `checked-before-merge`, `only-what-changed` |
| Releasing | tags, packaged versions, release branch, manual production job, docs sites, drift audit, database roles | `numbered-versions`, `back-in-seconds`, `docs-match-version`, `one-team-not-ready`, `production-waits`, `no-secret-fixes`, `right-keys-only` |
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
| Code a demo commits to GitLab | `patches/<name>/_patch.yaml` (`title`, `message`, `ops` with `path` / `find` / `replace`, or full files) |
| What the demo showed last time | `recordings/<id>.json`, written by every successful live step |
| Actions, evidence, cleaners | `backend/studio/actions/*.py`, `backend/studio/evidence.py`, contract in `actions/base.py` |
| Plain words for technical names | `backend/studio/names.py` |

`scenarios/` and `patches/` are mounted read-only into the container and read at start-up: after an edit, restart
with `make up S=workshop-studio`. A file that does not validate stops the app from starting.

## 3. The file

Copy the closest existing scenario. Skeleton with the limits the catalog enforces:

```yaml
id: only-what-changed            # = file name
group: changes                   # a group id from _groups.yaml
title: ...                       # <= 60 characters, no [[term]]
promise: ...                     # <= 110 characters: the benefit in one sentence
minutes: 3
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

code:                            # one to three blocks
  - title: ...                   # plain words
    file: models/.../x.sql       # in the dbt project; singlestore/<file>.sql or dev-image/Dockerfile also work
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
      say: ...                   # at most two short sentences, no duration (the app shows "Usually about N s")
      button: ...                # says what will happen
      inputs: []                 # day | number | choice | toggle, each with label, default and help
      do: [ {action: ...}, ... ]
      show: [ {type: ..., title: ..., caption: ...}, ... ]
      outcome: ...               # a sentence, or {check, good_when, good, not_quite}
```

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
   moves them to "Technical details"; scenario text must not bring them back. Words that cannot be avoided go into
   the glossary and appear as `[[term]]`, never in a title, promise or headline.
5. **Part 1** names the harm concretely: who, what they lose, how much. **Part 2** has exactly three sentences that
   do not repeat the headline, and a flow with exactly one `fix` box.
6. **Code blocks** show real instructions (not comment prose), at most 12 lines, starting and ending at a statement
   boundary, with 2 to 4 notes of at most 15 plain words. Never say the code is simple.
7. **Step text** is for the audience: no "Say:", no stage directions, no durations, no URLs. Captions tell the
   viewer where to look ("Look at the Development row").
8. **Counts in sentences use the words of the list next to them** ("1 failed, 2 not run", not "three failed").

## 5. Demo building blocks

Actions (`do:` and `setup:` items; each validates its parameters at load time):

| Action | Use |
|---|---|
| `feed.condition`, `feed.deliver` | make a feed late, paused, dirty, duplicated, or in another format; replay the last n days |
| `airflow.set_variable`, `airflow.delete_variable`, `airflow.run_dag`, `airflow.retry_failed` | operator settings and runs (`expect: success \| failed`) |
| `dbt.run` | a dbt command as the environment's robot, the CI robot or a developer |
| `sql.exec` | one statement as a named account; needs `undo` when it changes something; `expect: refused` for access demos |
| `gitlab.branch`, `gitlab.commit` (a patch), `gitlab.merge_request`, `gitlab.merge`, `gitlab.wait_pipeline`, `gitlab.play_job` | proposals, rehearsals, numbered versions, the manual production step |
| `git.revert_on_release`, `git.promote_release` | the release flow |
| `deploy.set_version` | point an environment at another packaged version |
| `precommit.run` | the formatter and rule checker on a patch |
| `docs.read_description` | what the published documentation of each environment says |
| `image.versions` | tool versions inside the developer toolbox and the production package |
| `mail.clear`, `wait` | housekeeping |

Evidence (`show:` items): `bars`, `number` (with `compare_to`), `table`, `tasks`, `pipeline`, `run_history`,
`freshness`, `versions`, `result` (plain lines, also from an action's `reasons` via `from_action`), `mail`. Any item
takes `title` and `caption`; `before_of: <step id>` shows the same item of an earlier step as the "Before" picture.

Clean-up is automatic: each action reports what it touched (`variable:dev:...`, `feed:...`, `git`, `version:prd`,
`models:dev`, `developer:...`, `sql`, `mail`, `landing:dev`), and the matching cleaner undoes it before the next
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
- Add unit tests in `backend/tests/` (fakes in `conftest.py`); run them in a throwaway container:
  `docker run --rm -v "$PWD/backend:/b" -w /b python:3.13-slim sh -c "pip install -q -r requirements.txt && python -m pytest -q"`.

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
  and its dependants, a description in a `.yml` file gave nothing, a project-wide `meta` gave 23 of 28 tables and a
  project-wide post-hook all 28. Measure it with a real proposal before writing a number or a reason into the text.
- A job log holds several `Processed:` lines; the one after `Finished 'build'` is the build.
- The run logbook (`<env>_vdp_mgmt.dbt_run_results`) is only written by Airflow runs. Do not use it to decide
  whether an environment is built.
- "Seeded" means the 90-day baseline is there, not "the table has rows": the background feeder adds today's rows to
  an emptied table within a minute.
- Code blocks can only read the dbt project, `infrastructure/singlestore/` and `infrastructure/dbt-singlestore-dev-image/`.
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

For a real check, give the YAML, the recording and the captured screens to a reviewer with a persona (some
technical knowledge, no data background) who must not edit anything and must return findings as
`id | severity | scenario | field | what is wrong | REPLACE WITH: exact text`, plus a verdict per scenario and a
short KEEP list. A second reviewer with a different job (finance, customer service, product) finds different things.
Apply the findings with a separate pass, re-record, and capture again.
