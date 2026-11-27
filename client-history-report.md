# The real dbt-vdp history: what happened, and what it means for the scenarios

Analysis of 2026-11-26 of the full git history of the client's dbt project (`dbt-vdp`, export
`~/Downloads/dbt-vdp-airgap-2026-11-26.tar.xz`): 4,217 commits on 203 refs, 1,125 tags, 2025-02-26 to 2026-11-26. The
question: which data domains, releases, reverts, repairs and growth are real, so that the Workshop Studio scenarios and
the quiz can be aligned with what actually happened. What was changed in the Studio as a result is listed in
section 11.

**Result in one paragraph.** The history backs almost every scenario with a real occurrence, several of them sharper
than the invented story: 61 features were taken out of releases by hand, 15 of 20 in one fortnight; one fix went out of,
into, out of and into production releases; a house rule with a typo caught nothing for 97 days; one table took
eight hours in production before anyone measured it; a production switch-over date was written two weeks too early
and went out in a release before it was corrected. Five
status labels ("in place today" / "next step") no longer match the repository (section 8). Section 7 lists, per
scenario and per flow stage, the real occurrence and the proposed tuning.

**How it was made.** I analysed the release branches, tags and reverts myself. Four Sonnet analysts each covered one
area (macros and growth, data domains, CI/Airflow/tooling, incidents); their full reports are in `client-history/`
(A to D). I re-checked the claims this report relies on against the repository (file counts, diffs, dates, whether a
commit is on `dev`, in a release, on `main`); corrections are in section 10. Numbers marked "about" come from keyword
classification and are approximate.

**Conventions.** No personal names, user names, addresses or host names. Ticket ids (`INTEL-…`), short hashes and
model names are kept as evidence. Environments in the client's words: LDV (development), ABT (acceptance), CBT
(pre-production), PRD (production). "Release nn" is the client's `2026_DROPnn`.

## 1. The project in numbers

| | 2025-06 | 2025-09 | 2025-12 | 2026-03 | 2026-06 | 2026-09 | 2026-11-26 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Models (`.sql`) | 77 | 227 | 1,005 | 1,125 | 1,431 | 1,736 | 1,963 |
| of which business vault (`edw_dv_bdv`) | – | 135 | 589 | 639 | 786 | 965 | 1,089 |
| logical model (`edw_ldm`) | – | 18 | 191 | 243 | 268 | 306 | 341 |
| dimensional (`im_dimensional`) | – | 39 | 89 | 102 | 219 | 281 | 315 |
| data products (`pres_dataproduct`) | – | 44 | 127 | 126 | 141 | 165 | 199 |
| Macro definitions | 16 | 24 | 44 | 53 | 59 | 67 | 64 |
| Data checks in yml | 14 | 160 | 725 | 1,210 | 2,017 | 3,653 | 5,015 |
| of which can stop a run (`severity: error`) | 0 | 0 | 0 | 0 | 96 | 598 | 1,592 |

- 3,548 commits on `dev`, 1,038 distinct tickets, 24 distinct author identities; 15 to 19 developers touch the vault
  layer every quarter. No separate team per layer: people work across layers per ticket. No CODEOWNERS, no owner on any
  model; seven "groups" with a named owner cover 111 of 1,963 models.
- The fourth quarter of 2025 added 778 models in three months (an import of a parallel line of work on 2025-10-09,
  `4ce61ff4`, 432 files; 143 logical-model tables on 2025-12-11, `0724739e`).
- About 22% of all non-merge commits are repairs (529 of 2,345 after de-duplication); the share rose from 11-14% in 2025
  to 26% in 2026 Q2 and 37% in 2026 Q3 (that quarter includes 44 release reverts and two restructurings).
- Checks grew 2.5 times between March and November 2026, faster than models (1.6 times). Two out of three still only
  warn: the project default is `warn` since 2025-10-09 and has never changed.
- Version numbers: about 1,090 numbered versions in 18 months (one per merged ticket; 116 in September 2026, about four
  a day) plus 35 release tags.

## 2. The real data domains

The organisation is a public employment service; the warehouse is a Data Vault on SingleStore. dbt builds four layers on
top of a raw vault that another tool lands (651 raw tables from Kafka topics): business vault (`edw_dv_bdv`: hubs,
links, satellites, reference tables, point-in-time tables), logical model (`edw_ldm`, pairs `_ab` current and `_hb`
history), dimensional model (`im_dimensional`: dimensions and six fact families) and data products
(`pres_dataproduct`). Full decoding of prefixes and suffixes: `client-history/B_data_domains.md`, section 1.

| Domain (client's word) | Plain meaning | Models (vault / logical / dimensional / product) | First seen |
|---|---|---|---|
| burger | citizen, jobseeker | 211 / 112 / 38 / 89 | 2025-03 |
| vacature | job vacancy | 118 / 13 / 49 / 23 | 2025-04 |
| competentie, soft skills | competences, competence reports | 113 / 43 / 2 / 14 | 2025-05 |
| opleiding, cursus, stage | training, courses, internships | 108 / 51 / 1 / 34 | 2025-03 |
| beroep, C2-conversie | occupations and the move to a new occupation standard | 102 / 34 / 5 / 42 | 2025-07 |
| organisatie, werkgever | employer organisations (plus an external company-data supplier) | 102 / 41 / 34 / 3 | 2025-08 |
| refkader | reference codes (municipality, country, study) | 76 / 0 / 0 / 0 | 2025-04 |
| IBO | individual training contract with an employer | 70 / 8 / 24 / 2 | 2025-10 |
| sollicitatie, jobbeurs | job applications, interviews, job fairs | 59 / 2 / 15 / 12 | 2025-04 |
| dienstverlening, sluitend maatpak | services delivered to a jobseeker | 53 / 12 / 26 / 3 | 2025-09 |
| interne organisatie, medewerker | own staff, teams, clusters | 52 / 9 / 34 / 3 | 2026-02 |
| niet-inzetbaarheid, beschikbaarheid | (un)availability of a jobseeker | 37 / 10 / 1 / 2 | 2025-04 |
| arbeidsbeperking | work-disability indication | 23 / 16 / 0 / 1 | 2025-10 |
| BIT | benefit-related track and status | 22 / 6 / 32 / 0 | 2026-04 |
| transmissie, controledossier | case transmissions; benefit-control files | 41 / 14 / 0 / 0 | 2025-10 / 2026-07 |
| afsprakenbundel | bundle of agreements per jobseeker | 15 / 10 / 0 / 8 | 2026-09 |
| indicatoren, APIM | KPI reporting; API usage | 3 / 0 / 55 / 4 | 2025-10 / 2026-05 |

Domains overlap, so columns do not add up to 1,963.

**Who reads it** (from the grants in the model files): AI and matching teams (81 data products for job matching,
"kans op werk", occupation search, a talent API), monitoring (the most-granted reader role, 456 mentions), HR planning
(420), the old data warehouse (34 integration tables), finance, an apprenticeship tenant. No exposures are declared.

**Where it comes from.** Event streams on Kafka (181 models read a Kafka timestamp; "crud" topics are the newer feeds
that replace older "summary" topics), plus frozen deliveries from predecessors: the first platform version, the old data
warehouse, and a SAS monitoring history. No source has a freshness rule: 0 occurrences of `freshness` or
`loaded_at_field` in 14 source files.

**Feeds that changed on a day.** Nine variables in the project file hold "from this moment, read the new feed", one
timestamp per environment. They are the real counterpart of `switchover-date`:

| Feed | What changed | Dates (LDV / ABT / CBT / PRD) | Story |
|---|---|---|---|
| Employer labels (`INTEL-23700`) | own topic to version 3 of the organisation topic | 03-25 / 03-25 / 03-24 / **03-24, corrected to 04-08** (2026) | production switched two weeks later than written; the wrong date stood from 2026-05-04 to 05-27, went out with release 12 and was corrected by a commit on the release 13 branch (`552e93d7`) |
| BIT (`INTEL-23866`) | archive of the first platform to the live feed | 2026-03-09 / 03-10 / 03-19 / 03-23 | a comment in the model: the archive stops on 2025-08-05, the date says March 2026, "what lies between falls away": 110,568 raw rows over 52,355 cases. Six models share the date |
| Vacancy occupations (`INTEL-27758`) | old topic to the crud topic with the new standard | 2025-04-29 / 05-13 / 06-03 / 05-28 | the date was only written into the project on 2026-11-23, about 17 months after the switch |
| Municipality and country codes | corrections in the reference source | PRD 2026-05-27, CBT 06-16; PRD 06-11, CBT 07-14 | a correction reaches environments weeks apart |
| Monitoring per jobseeker (`INTEL-20932`) | delivered SAS history to the platform | 2025-07-01 for all | a frozen delivery, so one date |
| Unemployment insurance; work-disability indication | summary topic to crud topic | PRD 2026-04-21, others 04-03; 2026-11-17 for all | the second one was committed as a future date: a planned switch |

All of these dates live in code: changing one is a commit and a release, not an operator setting.

## 3. How the project and the macros grew

Timeline (details and the per-macro inventory: `client-history/A_macros_and_growth.md`):

| When | What |
|---|---|
| 2025-02 to 04 | A template: one model file, four macros. |
| 2025-05 to 07 | First 77 models in a temporary layout, re-laid out into the four layers in July. First hashing, key and timeline macros. |
| 2025-10-09 | Import of a parallel line of work: 432 files, macro files 24 to 42, first sub-folders. The catalogue target was lost in this merge and restored 19 days later (`64f145ce`). Default check severity set to `warn`. |
| 2025-10-31 | One date literal written the old database's way replaced in 322 files (`597e2486`). |
| 2025-11-24 to 12-03 | "Standardisation" of 333 models merged into the production branch, reverted the next day, merged again, reverted again, settled by an 8-file fix (`5fd40569`, `18dee2ff`, `bfd031ad`, `3a52f715`, `5b9e5003`). |
| 2025-12-04 | `dev` becomes the integration branch; three rehearsal jobs (feature to dev, dev to release, release to main). |
| 2025-12-22 | Formatter switched on for everything: 887 files in one commit (`70f83680`); test arguments moved to the new syntax. |
| 2026-02-13 | Access rights moved from SQL into each model's yml: 327 yml files created (`3961c734`). |
| 2026-03 to 06 | Engine 1.9 to 1.10 (03-30) to 1.11 (06-22); the last upgrade deleted five adapter work-around macros. |
| 2026-05-15 | Incremental framework (six macros). The project was renamed and renamed back the same day; the commit notes that a change to the project file makes everything count as changed. |
| 2026-08-05 | 2,186 files moved to folders named after vault type, 57 changed lines (`2027a1dc`). |
| 2026-09-04 / 09-09 | Forced reformat again: 843 SQL files, then 213 yml files. |
| 2026-09-17 | Rebuild-next-to-the-live-table target, its helper macros and the first selectors (`762ddb4f`). |
| 2026-10-27 | Macros reorganised into 11 documented folders; macro lines 2,254 to 3,940 without new logic. Preparation for the next engine. |
| 2026-11-19 / 11-20 | A primary-key check with severity by environment; a rebuild list (`full_refresh_models`) next to the six per-table switches. Both first ship in release 26. |

Macro families and what their history says:

- **Overrides of engine internals** (`generate_alias_name`, schema and database name, table and view creation, grants):
  written as work-arounds, six changes to the naming rule, five work-arounds deleted by an engine upgrade. A project
  that leans on overrides pays at every upgrade.
- **Hashing** (`generate_md5_hash`, 12 changes): the macro where a small change touches every satellite.
- **Rebuild gates**: one macro plus one switch per large history table, six times between 2025-10 and 2026-11, then one
  list. The switches were never flipped in the file (0 flips found): a rebuild is asked on the command line.
- **Tidy-up**: drop a developer's own copies (2025-07), drop what a rehearsal built (2025-08), find objects no model
  owns with a dry run (2025-12), database maintenance (2026-03).
- **Run records**: the hook that writes every run to a table (2025-09-23) took five commits in a week to settle and
  later gained the compiled SQL (2026-08-17).

## 4. Releases as they really went

**The mechanics.** Until November 2025 every merge went into `main` and got a version number. From December 2025 work
merges into `dev`; a release branch is cut from `dev`; what is not ready is reverted on the release branch; the branch
is merged into `main` with a release tag (`.0`), and every later fix gets a patch tag (`.1`, `.2`, …). Since June 2026
a release goes out every second Thursday. Release branches are never merged back into `dev`, and `main` is rebuilt for
every release (34 of 35 release tags point at commits no branch contains any more).

| Release | Cut from `dev` | Features merged since the previous cut | Taken out | Tags |
|---|---|---:|---:|---|
| 12 | 05-12 | – | 0 | .0 and .1 on 05-19 |
| 13 | 05-26 | 31 | 1 | .0 05-28 |
| 14 | 06-09 | 36 | 2 | .0 06-09, .1 06-11, .2 06-15 |
| 16 | 07-03 | 65 | 2 | .0 07-09, .1 07-10 |
| 17 | 07-17 | 20 | 17 (15 new, 2 carried over) | .0 07-23, .1 07-24, .2 and .3 07-30 |
| 18 | 07-31 | 37 | 1 | .0 08-06 |
| 19 | 08-19 | 44 | 13 | .0 08-20, .1 08-21 |
| 20 | 09-03 | 30 | 9 | .0 09-03, .1 09-08 |
| 21 | 09-11 | 51 | 2 | .0 09-17, .1 09-22 |
| 22 | 09-29 | 40 | 6 | .0 10-01 |
| 23 | 10-15 | 45 | 5 | .0 10-15, .1 10-20 |
| 24 | cut from release 23, not from `dev` | 0 | the same 5 | .0 10-29, .1 and .2 11-09 |
| 25 | 11-12 | 75 | 6 | .0 11-12, .1 11-25 |
| 26 | 11-24 | 44 | 4 | .0 11-26 |

All dates 2026. "Taken out" counts distinct features; a feature merged several times needs one revert per merge (101
revert commits in total). Script: `client-history/release_stats.py`.

What the table and the commits behind it show:

1. **61 distinct features or fixes were taken out of a release**, out of about 520 merged in those windows. The reverts
   of one release carry the same timestamp to the minute: it is a batch on release day (release 17: 2026-07-23 09:49;
   release 19: 2026-08-19 11:29; release 20: 2026-09-03 07:01).
2. **Release 17 took out 15 of the 20 features merged in that fortnight**, plus 2 held over from release 16.
3. **Taking out has to be repeated.** `dev` keeps the feature, so the next release branch contains it again. 6 items
   were taken out of two releases, 2 items out of four: `feature/INTEL-26761` (releases 23, 24, 25 and 26, so 6 weeks
   and still out) and the fix for the history of "not employable" reasons (`INTEL-24181`).
4. **One fix went out, in, out, in.** Checked by file content of `dp_ai_burger_niet_inzetbaarheid_hb`: the fix is absent
   in releases 13 and 14, present in 16 and 17, absent again in 18 and 19, present from 20. Nothing in the history says
   whether 16 and 17 were meant to carry it.
5. **A revert is not always clean.** Release 19: two minutes after reverting that fix, a manual commit had to delete a
   description file the revert left behind (`de7a15cb`, "Fixed compilation warnings"); a key fix followed the next
   day (`.1`).
6. **The same feature merged again and again.** `feature/INTEL-24469` was merged into `dev` five times in seven days and
   then reverted four times on release day; `feature/INTEL-26342` four merges, four reverts.
7. **Most releases needed a repair after going out.** 16 patch tags on 12 of the 18 tagged releases (10 of the 14 in
   the table). Examples: release 21 went out on 09-17, the fix for the eight-hour production run came on 09-22;
   release 24 went out on 10-29, two production bugs in the vacancy conversion were fixed on 11-06 and 11-09; release 26
   needed a fix to the job files on the day of its tag.
8. **Every production fix is made twice**, once on the release branch and once on `dev`, by hand (same subject, other
   hash). Most were. Not on `dev` in the same form: five commits, among them today's fix on release 26 (`d25bea4d`).
9. **Earlier releases show the same shape**: on 2026-01-07 four features were taken out of release 03 in one day; one
   came back six days later.
10. **Tooling is catching up**: since release 25 a generated change log per release lists "Added (40)", "Excluded (4)"
    with a reason, and "Previously reverted, now accepted (4)".

## 5. What went wrong, and how it was repaired

Repair commits by kind (keyword classification of subjects, approximate; full table and 20 documented stories in
`client-history/D_incidents_and_fixes.md`): reverts and accidental commits 86, duplicate or missing keys 36, a feed or
source format that changed 33, a forgotten file or yml 31, performance and memory 26, history or timeline wrong 24,
wrong filter or rule 14, tooling 13, rebuilds 8, production-only 8, unspecific 250.

The stories with the clearest cost:

| # | What happened | When, evidence | Cost |
|---|---|---|---|
| 1 | **The eight-hour table.** One table took about eight hours to build in production: three small link views were recomputed every time a large table was built. Fix: 15 lines that store them as tables. | subject "8u op PRD", 2026-09-22, `19c58a28`; one of the views dates from 07-24 | about 60 days of slow loads (inferred), found in production five days after release 21 |
| 2 | **A 333-file change merged to the production branch twice and taken back twice.** | 2025-11-24 to 12-03 | four big merges and reverts in nine days |
| 3 | **Double rows in three data products in two weeks**, reported by the consumers; only then one generic primary-key check was written. | 2026-10-23 to 11-06 (`INTEL-27136`, `27178`, `27187`); check 2026-11-19, `ec267008`, used by 0 models so far | the same class of bug repaired table by table for months |
| 4 | **One fact table repaired nine times in ten weeks**: missing keys, slowness, a rewrite to incremental, doubles, window errors. | `t_fagd_ftr_vacature_indicator`, 2026-08-21 to 10-30 | each repair caused the next |
| 5 | **Out of memory for seven months** on the vacancy history table: April, August, September; an initial-load script, an incremental twin, finally a rebuild in chunks next to the live table (still on a branch). | 2026-04-21 to 11-10 (`fd2813f8` … `7747c6fa`) | memory failures showed up in acceptance and pre-production first |
| 6 | **Checks silenced to get a green run.** Four `not_null` checks commented out when they failed in production only. | 2025-10-24, `05f7cb34`; about 100 commits remove, disable or downgrade checks, peak autumn 2025 and 2026 Q3 | the structural answer (severity by environment) came 13 months later |
| 7 | **The house rule that caught nothing.** The pattern for the forbidden date function read `tstr_to_date`. | added 2026-08-19 (`7ec28296`), repaired 2026-11-24 (`0af07434`); the repair found three models still using the function | 97 days of false safety; the rules run only on the developer's machine, never in the shared pipeline |
| 8 | **Production's switch-over date was written two weeks too early.** | section 2, employer labels; in the project for three weeks, in one release | corrected on a release day |
| 9 | **The load asked "which environment am I?" and fell back to development** when the host was unknown. | fixed on the day before release 13, 2026-05-27, `223f85e9` (`hosts.get(hostname, "LDV")`) | per-environment dates and settings depend on that answer |
| 10 | **The switch-off list had to be repaired.** After the move of the load to pods the list of broken tables was written in a way that could not work; the fix passes it as an exclusion. | 2026-07-17, `84844d88` (what exactly happened in the nights before is inferred from the fix) | fixed on release 17 |
| 11 | **A commit by accident on the production branch.** | 2025-09-12, `e17823b8`, reverted the same day | none visible; nothing prevented it then |
| 12 | **The image recipe edited 37 times in the web editor**, 13 times on one day, each a new version number. | 2026-02-27 to 08-14 | 23 numbered versions that are only image experiments |
| 13 | **Source declarations are the top repair spot**: 26 repair commits on the raw-vault source file ("fixed missing sources"). Sources were generated outside git until 2025-11-20. | section 4 of report D | a forgotten line breaks a run |
| 14 | **A gap between two systems**: 110,568 rows that would silently fall away. | section 2, BIT | found by a developer, noted in a comment |

## 6. Tooling timeline

| Area | What the history shows |
|---|---|
| Pipeline | GitLab CI from 2025-05-02 to 2026-02-18; since then a two-line `Jenkinsfile` that calls a shared library outside this repository, so today's rehearsal is not visible here. |
| Rehearsal | State-aware from the first pipeline (2025-05-06, tests on changed models only); a real build into a schema per branch with clean-up since 2025-08-26; data checks excluded from the feature rehearsal since 2025-11-28; the empty first round removed (2025-11-28), added (12-11) and removed again (12-16) for an adapter bug. |
| House rules | Formatter, parse and rule checker only as local hooks. Rules: naming (2025-03), semicolon and hard-coded references (2026-06-18), forbidden functions (2026-08-19, repaired 2026-11-24). A secret scanner since 2026-10-02. |
| Developer setup | A dev container from day one; image version pinned since 2025-07-14; every developer builds under their own name; VS Code tasks "Build modified", "List modified", "Replicate CI pipeline locally" since 2026-05-20. The developer's password is typed once and kept in a local untracked file. |
| Nightly load | First job 2025-05-23 with a switch-off list for broken tables; consumers' jobs start by themselves when their tables refresh (2025-06-27); failure mail (2025-06-26); retry of only the failed part (2025-08-08, started by a person: no automatic retry); a "warehouse is ready" signal to the central scheduler (2025-09-05); Kubernetes pods with an image tag per environment (2026-05-12); start on "the data is in" in place of the clock, and a failure mail that is silent for manual runs (2026-05-27); heavy tables only on demand (2026-06-09); a selector for the nightly list (2026-10-19, fixed again 10-23). |
| Release job | Created 2026-07-31; a hand-edited step list (a rebuild of one history table in seven batches, 2026-08-04); rewritten 2026-11-23 to run listed statements one by one, pausing the nightly load meanwhile; first real list for release 26: maintenance, the nightly build, one on-demand table, three rebuilds. |
| Documentation | A docs image built from the same build as the package, per environment, since 2026-01-30; a generated front page with release, build time and commit since 2026-11-06 (`c5dbc5df`, in release 25). |
| Versions | `v1.x` (2025-05 to 2026-01), `1.x` (to 2026-06-02), `2.x` (since 2026-06-03); the reason for the two renumberings is not in the repository. Images are tagged per environment, not per version. |

## 7. Scenario by scenario: the real occurrence behind it

Fit: **strong** = the same thing happened; **partial** = a neighbouring thing happened; **none** = the history is silent.
"Tuning" is what I propose to change in the scenario or its question for the room.

### When the data changes

| Scenario | Real occurrence | Fit | Tuning |
|---|---|---|---|
| `switchover-date` | Nine switch-over dates per environment; production's date two weeks too early and shipped that way; a gap of 110,568 rows; a date written down 17 months late (section 2). | strong | The real risk is not "we cannot switch" but "the date is wrong or missing". Let the platform option's impact carry that price, and name the next step precisely: the date as an operator setting, changed without a release (today it is code). See section 8. |
| `late-delivery` | No freshness rule on any of 14 source files. The night waits for its inputs since 2026-05-27, and nothing reports an input that stays away. | strong | Keep `next`. The option "wait for everything" is close to how the night behaves today when an input job is missing; "load with what is there" is how it behaves when the job ran on a silent topic. Say both in the impacts. |
| `skip-one-table` | The switch-off list exists since the first nightly job (2025-05-23). On 2026-07-17 it had to be repaired: after the move of the load it no longer worked as intended. | strong | Add the price to the platform option: the list is only worth something if someone proves it switches off. |
| `strict-where-it-matters` | Default `warn` since 2025-10; 5,015 checks, about a third can stop a run; checks commented out when production failed (2025-10-24); severity by environment arrived 2026-11-19 and no check uses it yet. | strong | Keep `next` (arriving with release 26, not wired in). The option "soft everywhere" is today's default for two out of three checks: its impact may say so. Add the real harm: a check that is silenced, not softened. |

### When the code changes

| Scenario | Real occurrence | Fit | Tuning |
|---|---|---|---|
| `same-toolbox` | Dev container from the start, version pinned 2025-07-14; three engine upgrades, one with an explicit "everyone must update the toolbox" message. | partial | The story (a newer tool on one laptop) holds. |
| `own-workbench` | Own name per developer from the start; a clean-up macro for a developer's copies (2025-07-24); an orphan finder with dry run (2025-12, 2026-03). | strong | None needed. The clean-up half of the idea has the richer history: three tools in eight months. |
| `house-rules` | The forbidden date-function rule with a typo, 97 days; 887 + 843 + 213 files reformatted in three big-bang commits; 322 files for one date literal; rules only on the developer's machine. | strong | The story (a date function that slips through) is the real one. Add to the platform option's impact that a rule nobody tested can do nothing, and link to the flow stage that proves a rule refuses. |
| `checked-before-merge` | Double rows in three data products in two weeks; a 333-file change merged and reverted twice; a branch merged five times in a week; data checks left out of the rehearsal since 2025-11-28. | strong | "Every order three times" is the webshop twin of the double rows. The honest today/next split is already in the flow `rehearsing-a-change` (stage "wrong"). |
| `only-what-changed` | State-aware from the first pipeline (2025-05-06); a change to the project file made everything count as changed (2026-05-15). | strong | Add that second fact as the price of the platform option: some small-looking changes are big. |

### When we release

| Scenario | Real occurrence | Fit | Tuning |
|---|---|---|---|
| `numbered-versions` | About 1,090 numbers in 18 months; the scheme changed three times. | strong | None to the story. For the presenter: the number exists per merge; what is missing is the environment saying which number it runs (flow `from-change-to-package`, stage "copy"). |
| `back-in-seconds` | No roll-back in the history: 16 patch releases on 12 of 18 releases; five days between release 21 and the fix for the eight-hour run; images tagged per environment, not per version. | strong | Keep `next`. The option "repair the new code while it stays live" is how it goes today; its impact may say so. |
| `docs-match-version` | Documentation built from the same build as the package, per environment, since 2026-01-30. | strong | The label `next` looks outdated: section 8. |
| `docs-front-page` | A generated front page with release, build time and commit since 2026-11-06, shipped in release 25. No counts of tables, checks, feeds or teams. | strong | The label `next` is outdated for the version and the date: section 8. |
| `one-team-not-ready` | Section 4, points 1 to 6 and 9. | strong | The scenario shows the good half. Add the real price to the platform option: what is taken out stays in the shared branch and has to be taken out again at the next release, by hand; one fix went out, in, out and in. Mirror the real shape in the situation: release day, several changes, a batch. A natural next step for the closing page: choosing what goes into a release, not removing it afterwards. |
| `production-waits` | Development automatic on every number, acceptance scheduled, pre-production and production manual with the newest release tag (pipeline of 2025-12-04). | strong | None. |
| `no-secret-fixes` | Manual database changes leave no trace in git by nature. The same class in code: every production fix made twice; a fix of today not yet on `dev`; the catalogue target lost in a merge for 19 days. An orphan finder exists; it does not detect a changed table. | partial | Keep `next`. A second real example for the room: a fix made on the release that never reaches the shared branch disappears at the next release, exactly like the hand-added column. |
| `right-keys-only` | The load that fell back to development when it did not recognise its host (2026-05-27); a commit by accident on the production branch (2025-09-12); access rights as code for 327 tables in one change (2026-02-13). | partial | The story (a setting copied to the wrong place) has a real twin: "unknown means development". Use it in the situation. |

### Every day

| Scenario | Real occurrence | Fit | Tuning |
|---|---|---|---|
| `one-clear-mail` | Failure mail since 2025-06-26; since 2026-05-27 silent for manual runs so that people who are testing do not flood the mailbox. | strong | Add the real refinement as the price of a mail for everything: a mailbox full of test failures is no longer read. |
| `redo-only-what-failed` | Since 2025-08-08; a person starts the retry, there is no automatic one. | strong | None. |
| `rebuild-without-downtime` | Seven months of memory failures on one history table; a rebuild in seven batches during a release (2026-08-04); the rebuild-next-to-live target on `dev` since 2026-09-17, the job and the chunked version only on branches. | strong | Keep `next`. The real pain is twofold: readers see a half table, and the rebuild does not fit in memory in one go. The second reason is missing from the story. |
| `every-run-on-record` | Run records since 2025-09-23; the eight-hour build of one table was found in production, not by the records. | strong | The story's last sentence (a table that gets a little slower every week) is that eight-hour table. Use it as the situation. |

### The five flows, per stage

| Flow, stage | Real occurrence | Label now | Fit |
|---|---|---|---|
| `rehearsing-a-change` / shared, everything, nocleanup | Schema per branch, state-aware build and clean-up since 2025-08-26. | today | strong |
| … / alldata (empty first round) | Removed, added and removed again in three weeks (2025-11-28 to 12-16) for an adapter bug in the old pipeline. | today | strong, with an anecdote: a safeguard you cannot trust gets switched off |
| … / failed (clean-up after a failure) | The clean-up step was dropped from and re-added to pipelines (2025-10-30, 2026-05-19). | next | partial |
| … / wrong (second round with data and checks) | Data checks excluded from the feature rehearsal since 2025-11-28; the double rows of autumn 2026 reached consumers. | next | strong |
| `developers-toolbox` / guard | Not found in this repository. | today | none |
| … / settings | One connection file, user and password from the environment, from the start. | today | strong |
| … / vault | In this repository a developer's password is typed once and kept in a local untracked file; vault roles are for robots and image builds. | today | weak: see section 8 |
| … / feed, update | Source declarations generated outside git until 2025-11-20; today the top repair spot (26 repair commits). | today | strong |
| … / buttons | Tasks "Build modified", "List modified", "Replicate CI pipeline locally" since 2026-05-20. | today | strong |
| … / rule (prove a rule refuses) | The typo rule, 97 days. | next | strong: the stage's story happened literally |
| `from-change-to-package` / number, package | Section 6, versions. The package recipe was edited 37 times in a web editor, 13 times on one day; 23 version numbers point at such edits. | today | strong |
| … / catalogue | The catalogue target, added 2025-09-18, lost in a merge and restored. | today | strong |
| … / copy (an environment says which version it runs) | Images tagged per environment. | next | strong |
| `guards-inside-every-run` / names | The naming rule, six changes. | today | strong |
| … / practice | The guard learned about empty runs on 2026-06-05 (`82656b67`, "models breaking the empty flag"). | today | strong |
| … / lingering | Orphan finder and maintenance, 2025-12 to 2026-06. | today | strong |
| … / written, logbook | Run records since 2025-09-23, written only for target `run`. | today / next | strong |
| … / refresh (rebuild only the listed tables) | Six per-table switches, then one list on 2026-11-20, first shipped in release 26. | next | strong: see section 8 |
| `the-night-steered-from-airflow` / list, request | Selector for the nightly list 2026-10-19 (fixed twice after release 23); on-demand tables since 2026-06-09. | today | strong |
| … / leave_out | As `skip-one-table`. | today | strong |
| … / landed | Start on "the data is in" since 2026-05-27; that change shipped on a release branch the day before the tag and needed two fixes the same day. | today | strong |
| … / told, failed | Consumers' jobs start by themselves when their tables refresh (since 2025-06-27). A mail to readers was not found. | today | partial: see section 8 |
| … / silence, retry | No warning for an input that never arrives; no automatic retry. | next | strong |

## 8. Status labels the history contradicts or sharpens

The labels say "in place at the client today" or "next step" and feed the closing page. Five deserve a decision:

| Where | Label | What the repository shows | Suggestion |
|---|---|---|---|
| `docs-front-page` | next | A generated front page naming release, build time and commit is on `dev` since 2026-11-06 and in releases 25 and 26. It has no counts. | `today` for version and date; the counts are the next step. This contradicts what was assumed on 2026-11-21, when the older export was the reference. |
| `docs-match-version` | next | The documentation is generated in the same build as the package and shipped as an image per environment since 2026-01-30. | Probably `today`. The repository cannot show whether each environment really serves its own image; confirm before changing. |
| `guards-inside-every-run`, stage "refresh" | next | The rebuild list is merged (2026-11-20) and in release 26, tagged 2026-11-26, not yet on `main`. | `today` once release 26 is live; `next` until then. |
| `switchover-date` | next | Switching on a date per environment is in place since 2026-05 (and in a cruder form since 2025-04). Not in place: changing the date without a release. | Keep `next` and name the feature after the part that is new (the date as a setting), or mark it `today` and move the setting to the closing page as an extra. |
| `developers-toolbox`, stage "vault" | today | In this repository the developer's password is not fetched from a vault. | Check against the toolkit outside this repository; if developers do not use the vault yet, this is `next`. |

Three labels are confirmed with a nuance worth saying aloud: `strict-where-it-matters` stays `next` (the macro is
there, no check uses it); `rebuild-without-downtime` stays `next` (branch only); `the-night-steered-from-airflow`,
stage "told", is `today` for jobs that start by themselves and unproven for a mail to human readers.

## 9. What the history shows that no scenario covers

Candidates, strongest first:

1. **One fix, one place.** Releases never flow back, so every production fix is typed twice and one of the copies can
   be forgotten (section 4, point 8).
2. **Choosing what goes in, not removing afterwards.** 101 revert commits, repeated every fortnight (section 4,
   points 1 to 4). This is the real "next step" behind `one-team-not-ready`.
3. **Rehearsing on production-sized data.** Memory failures and the eight-hour run appeared in the higher environments
   only; test data did not show them.
4. **A silenced check.** About 100 commits switch a check off or down; no scenario shows what a check that was
   commented out costs later.
5. **One kind of change per proposal.** 2,186 files moved in one commit with 57 changed lines worked because nothing
   else was in it; 887 reformatted files next to a syntax change did not review well.
6. **The release list.** What a release must do beyond the nightly build (maintenance, an on-demand table, three
   rebuilds) is a hand-edited list in a job file.
7. **Trying a change to the package recipe before it gets a version number.** 37 web-editor edits of the recipe, 23
   version numbers that are only such attempts.

## 10. Corrections and limits

- An analyst counted 39 "Edit dev.Dockerfile" commits; the repository has 37.
- An analyst wrote that the "not employable" fix was kept out of four releases; file content shows out, in, out, in
  (section 4, point 4).
- My first summary said release 17 took out 17 of 20; precisely it is 15 of the 20 merged in that window plus 2 carried
  over from release 16.
- An analyst wrote that production's switch-over date for the employer labels was wrong for seven weeks. The wrong
  value was in the project for three weeks (2026-05-04 to 05-27) and in one release; seven weeks is the time between
  the real switch and the correction.
- The first formatter commit is counted as 921 files on a side branch and 887 on `dev`; this report uses 887.
- Today's rehearsal and deploy steps live in a pipeline library outside this repository. Statements about them are
  limited to what the old pipeline file showed until 2026-02-18.
- Run times are not in git. "Eight hours" is the only duration, from a commit subject that names one table ("8u op
  PRD"); an analyst and my first summary called it an eight-hour night. Whether the reverts of
  2026-01-07 or the accidental commit reached production is not recorded.
- The reasons for holding a feature back are never in a commit message.
- Repair classification is by keyword on the subject; 47% of repairs are "unspecific".
- The environment names LDV, ABT, CBT and PRD are read as development, acceptance, pre-production and production;
  that reading is inferred.

## 11. What was changed in the Studio (2026-11-26)

Decisions of the owner the same day: the real occurrences enter the Studio as a separate line, with numbers and without
dates; two status labels change.

- **A new optional field `reality`** on a scenario and on a step (1 to 260 characters), shown under the heading "Bij ons
  gebeurd": after the answer of a question is revealed, in part 1 under the cost, and in the step card of a stage (once
  that stage's question was asked or skipped).
  Backend `models.py` and `catalog.py`, frontend component `Reality`, tests on both sides, the Studio README, the
  scenario skill (new section 4b with the rules) and the Flemish word list.
- **40 lines written**: 20 ordinary scenarios (all except `docs-match-version`, whose label would contradict its line)
  and 20 flow stages. A reviewer with the persona of a listener without a data background read them once; 18 were
  reworded, one duplicate was dropped. Each line states only what this report verified; lines that rest on an inference say less than
  the report does.
- **`docs-front-page` is now "in place today"**; counting tables, checks and feeds on the front page is a new "next
  step" row on the closing page. Two of its three code blocks carry the "today" badge; the counting block stays "next".
- **The stage "vault" of `developers-toolbox` is now a "next step"**, with a "Zonder" sentence, the quiz impact that says
  how it goes today, and the matching code block badge. The closing page no longer lists the vault login among what the
  older tool has.
- **Not changed** (not chosen): `docs-match-version` and the stage "refresh" of `guards-inside-every-run` keep "next";
  the feature name of `switchover-date` is unchanged. No story, quiz option or demo step was rewritten.
- **Checked**: all 26 scenario files validate with the new backend code; backend 583 tests and frontend 323 tests pass;
  the three places where the line appears were looked at in the mock frontend.
  Not done: a live run or a new recording (texts and labels do not change what a step shows), and a rebuild of the
  Studio image, which the running stack needs before it shows the new line.

## Appendix

- `client-history/A_macros_and_growth.md`: monthly growth table, every macro with first and last change, the ten macros
  that changed most, project file history, engine upgrades, the fifteen largest commits.
- `client-history/B_data_domains.md`: layers and suffixes, domains per layer and per quarter, sources, the switch-over
  variables with their models, consumers, checks per quarter, who works where.
- `client-history/C_ci_airflow_tooling.md`: pipeline, rehearsal step over time, house rules, images, the nightly and
  release jobs, version numbers, branching per quarter.
- `client-history/D_incidents_and_fixes.md`: repair classification per quarter, twenty documented stories, repeat
  offenders, environment differences, rebuilds, checks added and removed.
- `client-history/release_stats.py`: the script behind the release table (argument: a git wrapper bound to the export).
