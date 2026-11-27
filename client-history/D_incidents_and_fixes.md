# D. Incidents and fixes: what went wrong in real life, and how it was repaired

Scope: defects in models, data and day-to-day work (release-branch reverts only where they belong to a story).
Method: `g log --all --no-merges`, deduplicated by subject + author date (2,729 distinct non-merge hashes collapse to
**2,345** commits; the same fix often exists 2 to 5 times as cherry-picks on release branches). Scripts and tables are in
the analyst scratch folder (`classify.py`, `repairs.tsv`, `tests.py`, `testchg.tsv`, not kept).

## 1. Classification of repairs (approximate)

Rule set (`classify.py`, `python3 -I`): a commit is a **repair** when its subject matches one of: fix*, bug*, hotfix, fout*,
verkeerd*, herstel*, correctie/corrig*, dubbel*/duplica*, revert, OOM, timeout, memory, traag, performan*, ontbreek*,
vergeten, missing, forgot*, herlad*, reload, full refresh, prd, productie, "uur", "8u". Each repair gets the FIRST matching
type of this ordered list (keyword rules on the subject only, not the diff): revert/accidental > performance/memory >
full refresh > duplicate/missing keys > history/timeline > production/environment > feed/source format > forgotten
file/yml > wrong filter/business rule > tooling/CI > other. This is a keyword heuristic: the counts are indicative, and
"other" (47%) is a bucket of terse subjects such as "fix: fixed window". Revert counts also include release-branch
reverts of whole features (101 "Revert" subjects exist, 86 are here after dedup).

| type | 25Q1 | 25Q2 | 25Q3 | 25Q4 | 26Q1 | 26Q2 | 26Q3 | 26Q4* | total |
|---|---|---|---|---|---|---|---|---|---|
| other repair (unspecific) | 2 | 26 | 16 | 42 | 29 | 39 | 69 | 27 | 250 |
| revert / accidental commit | 0 | 2 | 1 | 6 | 6 | 7 | 44 | 20 | 86 |
| duplicate / missing keys | 0 | 2 | 3 | 0 | 1 | 9 | 10 | 11 | 36 |
| feed / source format changed | 0 | 3 | 4 | 6 | 2 | 7 | 6 | 5 | 33 |
| forgotten file / yml / config | 1 | 7 | 2 | 3 | 3 | 8 | 5 | 2 | 31 |
| performance / long runs / memory | 0 | 1 | 3 | 1 | 2 | 5 | 10 | 4 | 26 |
| history / timeline wrong | 0 | 1 | 1 | 1 | 0 | 5 | 13 | 3 | 24 |
| wrong filter / business rule | 0 | 0 | 1 | 1 | 3 | 3 | 3 | 3 | 14 |
| tooling / CI / formatting | 1 | 5 | 0 | 3 | 1 | 2 | 0 | 1 | 13 |
| full refresh / rebuild / reload | 0 | 0 | 0 | 0 | 2 | 2 | 3 | 1 | 8 |
| production / environment only | 0 | 1 | 1 | 3 | 1 | 1 | 1 | 0 | 8 |
| **all repairs** | 4 | 48 | 32 | 66 | 50 | 88 | 164 | 77 | **529** |
| all non-merge commits (dedup) | 34 | 231 | 277 | 466 | 290 | 326 | 439 | 282 | 2,345 |
| **repair share** | 11% | 20% | 11% | 14% | 17% | 26% | 37% | 27% | **22%** |

*26Q4 = Oct to 26 Nov only. Reading: the share of repair commits rises from 11-14% (2025) to 26% (2026Q2) and 37% in
2026Q3. 2026Q3 is inflated by release-branch reverts (44), the August folder restructure and the September formatter/
incremental rework, so part of the peak is "release housekeeping", not new defects. Duplicate-key and history/timeline
repairs grow together with the new BIT, vacature-indicator and data-product layers (2026Q2 to Q4).

## 2. The stories you listed

**2.1 "fix: satc_sluitendmaatpak_hb 8u op PRD fix" (2026-09-22, 19c58a28 / 47f5dbba, ticket INTEL-26612).**
Three link views (`lksb_smp_sluitendmaatpak_bemiddelaar/burger/trusted_advisor_hb`) were `materialized: view`, so every
downstream build (satc_sluitendmaatpak_hb) re-evaluated their joins. Production took about 8 hours (the subject: "8u op
PRD"). The repair is 15 added lines of YAML: the three views become `table` with `shard_key: hub_sluitendmaatpak_hkey`, and
`satb_smp_sluitendmaatpak_hb` (an empty yml until then) gets a table + shard key. Merged to dev the same day (ab8e58a5),
now on release `2026_DROP21`. Introduced: the bemiddelaar link was created 2026-07-24 (ffb035cf, INTEL-25408), so about
60 days passed (inferred; the burger and trusted-advisor views may be older). Noticed by the nightly production load
(inferred from the subject), not by a user.

**2.2 "bigfix: Adding dummy keys for burger and vacature" (2026-08-21, 63197baa / 67546b7e, INTEL-26047, merge 498e94f8).**
The fact `t_fagd_ftr_vacature_indicator` held rows whose burger or vacature key was NULL because no matching dimension row
existed. The fix wraps the four keys in `coalesce(..., dummy)` and casts to `binary(16)`, pointing at the "unknown" row
(`vacature_uuid = '~UN~'`, valid from 1900-01-01). Its follow-up history is its own story (see 3.1): this one table had
nine fix commits in 10 weeks.

**2.3 The fix that was reverted four times: "AI (KoW): Historiek van deletes niet-inzetbaarheid klopt niet".**
Original a217d8f5 (2026-05-12, ticket INTEL-24181, merged to dev 2026-05-15, 98f63baf): 6 models around "niet-inzetbaarheid
reden" (`ldsc_burger_niet_inzetbaarheid_reden_hb`, `ldsb_ami_inzetbaar_reden_hb`, `refc_niet_inzetbaarheid_reden_definitie_hb`,
`dp_ai_burger_niet_inzetbaarheid_hb`, ...), the history of deleted records was wrong. Reverts, each on its own release
branch: b03c4c6a on `release/2026_DROP13` (2026-05-27), 8b51f4d1 on `release/2026_DROP14` (2026-06-09), cb62b4d8 on
`release/2026_DROP18` (2026-08-06), 68d0a2b6 on `release/2026_DROP19` (2026-08-20). Reason: because the fix sits on `dev`,
every release branch cut from `dev` inherits it; on four release branches the release manager took it back out (the
consuming team's side was apparently not ready; reasons are not in the commit messages, inferred). So it was kept out of
four releases but not in a row: DROP15, 16, 17, 20 and later contain the fix without a revert (branch --contains shows
it in 26 release branches). Cost: four reverts, four extra merge rounds, and a correct history that production did not
have for at least 3 months (May to August; whether it shipped later was not verified).

**2.4 "Revert 'INTEL-20532 - standaardisatie'" (2025-11-25, 18dee2ff) and its revert on main (2025-12-02, 3a52f715).**
INTEL-20532 was a project-wide standardisation: 333 files, +38,204/-7,619 lines (a0e7ed53, merged to `main` 2025-11-24,
5fd40569). Next day it was reverted (18dee2ff, 333 files again). It came back 2025-12-01 (14d162e0, 327 files, bfd031ad)
and was reverted again the next day (3a52f715). Only on 2025-12-03 a small bugfix branch with the same ticket id (cffc0d4f,
8 files, +191/-165) went to `main`. Two big-bang merges to `main` were taken back within 24 hours each. Earlier a
squash variant had also gone to main (84ef6f87, 2025-10-28). Lesson: a 330-file change merged straight into the
production branch.

**2.5 "revert commit (per ongeluk naar main ipv lokale branch)" (2025-09-12, e17823b8).** A developer committed
`satb_burger_taal_hb` (sql + yml, 48 lines, commit 313107b4 "Creer object satb_burger_taal_hb via dbt") by accident on
`main` instead of a local branch; the revert removed it the same day. No tests or CI prevented the commit on main at the
time (inferred; the CI target branch for prd was only changed on 2025-12-08, 29c8b0ef).

**2.6 "Revert 'Edit dev.Dockerfile'" (2026-02-27, 4a85eeff).** A one-word change in the developer image file, reverted
the same day. It is the visible tip of a pattern: **37 commits titled "Edit dev.Dockerfile"** (the default message of
the web editor), in bursts on 02-27 (3 + the revert), 03-02 (4), 03-10 (13), 04-21 to 04-27 (about 16), 08-06 (2).
The developer image was tuned by trial and error in the GitLab web editor, one pipeline per attempt. Category: tooling.

**2.7 Four reverts on 2026-01-07 (1adad4a8, 60fa96f9, 0ce827e5, 16a0d471).** Release candidate `2026_DROP03` (tags
2026_DROP03.0 and .1; the reverts are reachable from those tags only): features INTEL-21742
(`t_fpsm_burger_actieve_catwz`, 158 lines), INTEL-21724 (7 files), INTEL-19689 (`vac_competentie_ab`, 10 files) and
INTEL-19186 (burger bemiddeling, 694 deleted lines) were taken out on one day, so that the release could go. INTEL-21742
and 21724 had been merged to dev on 2025-12-17/18 and 12-23, three weeks before. INTEL-21724 came back on 2026-01-13
(87f978de). Shape: "one team not ready at release time".

**2.8 "fix(dbt): toevoegen index t_int_ai_beroepzoeker_predicties_ab" (2026-06-11, 8b98981e, on `release/2026_DROP14`).**
The yml changed the shard key from 3 columns to 2 (`tekst_hash`, `competent_release_nr`) and added a hash index on the
same two. A prediction table used for lookups on text hash without an index would scan; a classical "forgot the index"
repair. Made directly on the release branch (a hotfix in the release flow), not only on dev.

**2.9 "INTEL-24477 DBT BUG SATB_VACATURE_VACATURECRUD_BEROEP_HB" (2026-05-19, 2fcc5ab7, merged f934e98e).** A one-line
fix: `partition_by=["hub_vacature_hkey","versie","code"]` became `["hub_vacature_hkey","versie"]`. The history of
beroepen of a vacancy was cut into separate series per `code`, so the timeline broke into pieces (inferred from the
diff). One column too many in a window.

**2.10 "INTEL-25744 DBT BUG C2 Conversie vanaf begin van zijn creatie" (2026-07-29; 5 hashes, merge 69031279/cd3808c1).**
The C2 profession conversion (vacatures and IBO) was started from the wrong date. The fix adds a first step
`bepaal_eerste_record_van_satc_vacature_hb`: the earliest record per vacancy (the lesser of the satc row and the
"salc" load timestamp), so that the conversion starts at the creation of the vacancy. 5 files, +254/-38; the model also
lost its dead commented code. Sits in a chain of C2 tickets: process redesign April 2026 (INTEL-23616/23673),
CBT error + performance May (see 3.2), more performance 07-10, this bug 07-29, "mag enkel converteren 3_20" 08-03,
"geen initialisatie bij nieuwe IBO contracten" 08-05: 6 weeks of C2 repairs.

**2.11 "INTEL-26360 filter planvanaanpak naar int_mon_burger issue" (2026-09-08, 128bf3c1 and 4 copies, merge 7aaaf92b).**
`int_mon_burger_hb` excluded burgers whose "plan van aanpak" source row was flagged deleted
(`x_is_burger_planvanaanpak_bron_del_vlag = 'N'`); removing that second filter (4 lines) made them reappear. Wrong
filter: a deleted detail row removed a whole citizen from a monitoring product. Same fix on 5 branch lineages on one day.

## 3. Ten more stories (best per type)

**3.1 Vacature-indicator fact: nine repairs in ten weeks (duplicate/missing keys, then performance, then rebuild logic).**
Table `t_fagd_ftr_vacature_indicator` (+ the monthly `fagm` and pivot siblings; 12, 10 and 9 repair commits on the three
sql files, the top of the repeat-offender list). 2026-08-21 dummy keys (63197baa); 09-08 "performance fix" (21ae283d);
09-10 "Performance increase" (5ee896dd, 211 added lines); 09-11 refactored incremental loading (390a995c); 09-21
"Fixed doubles" (b01023a8, +150/-96); 09-23 adjusted incremental load (6c77739a); 09-25 three more (7a2667b3, 9f603493,
a3626edf, "fixed window"); 10-23 split JOB_VERVULD; 10-30 "VACATURE_BEWAARD gefilterd en incremental load fix" (1980fe5b);
11-12 two features touching it reverted on dev (4c2c194a, b6548d79). Pattern: a new fact, built as full rebuild,
became too slow, was rewritten incremental, and the rewrite produced doubles and window errors. Cost: 6 weeks of rework.

**3.2 C2 performance after a CBT error (performance; 2026-05-06, 11b9f499, INTEL-23993 "...NA CBT ERROR").**
In the acceptance-like environment CBT the C2 conversion method 03 for vacatures and IBO failed (memory). Fix: 2 lines per
model: `from straight_join satc_..._init_ab sat join ...` which forces MySQL-style join order. A second pass followed on
2026-07-10 (INTEL-25502, merged 07-27, 17 days later). Production was spared by catching it in CBT (inferred).

**3.3 satc_vacature_hb: seven months of memory fights (OOM, then incremental, then chunked rebuild).**
2026-04-21 "cutoff date and memory issues" (fd2813f8); 08-06 "Fixed OOM" (f9e5fe91, +74/-29); 09-08 "created the initial
loading script" (aea778aa); 09-25 "Initial load ... OOM fix" (6019b3cb, 291 lines changed, two helper models removed);
09-28 quote fix for incremental date parameters (6c1639c0); 10-09 "incremental implementation" (3c2030f5); 11-10 "Batched /
chunked full-refresh voor de zwaarste modellen" (7747c6fa): the model is rebuilt next to the live table in hash chunks
("zodat voor een full-refresh niet meer het geheugen van de volledige join nodig is"). Same pattern on
`dp_ai_burger_beroep_hb` (2026-07-02 4ad0020f materialise an intermediate, 07-07 1fdc7813 straight_join, 07-08
5e8f4232 "OOM") and `t_ftr_sollicitatie_activiteit` (2026-06-18 470b0d7c "OOM error in ABT": split off a `_puur` model).
Also 2025-08-13 `satc_burger_beroep_arbeidsres_hb` (cf2e12fc INTEL-19497 "OOM").

**3.4 "Dubbele rijen" in three data products in two weeks (duplicate keys; 2026-10-23 to 11-06).**
`dp_ai_burger_talenkennis_hb` (9bec21f6, INTEL-27136, +90 lines yml with not_null tests), `dp_ai_burger_voorkeur_regio_hb`
(57e1cbf2, INTEL-27178, 11-05), `dp_ai_burger_voorkeur_tijdsregeling_hb` (d9237c20, INTEL-27187, 11-06), plus
`lksb/satb_..indicatie_arbeidsbeperking` timelines (1fed2303, INTEL-27547) and the location master (d10c245b INTEL-27232
"fout in load door locatiemaster dubbels"). Consumers (data products for AI/job matching) reported rows twice. Then the
structural answer: a generic `primary_key` test with an environment-dependent severity (ec267008, 2026-11-19,
INTEL-27197). Same bug class repaired model by model for 6 months before a test was added to catch the class.

**3.4b NULLs in keys: INTEL-27030 (2026-10-16, 9ef94816 / 2e9c5f6a)** "vervang NULL-waarden bij codes/uuid's door
dummy-waarden", after 2026-10-06 (78fc3493) "default voor gebruiker_hkey als NULL" and 05-28 (1f0b6c87) dummy values for
`organisatie_hkey`. In a Data Vault the "~UN~" unknown row must exist for every key; it was forgotten in object after
object (2026-04-20, 05-06, 05-28, 06-17, 08-21, 10-06, 10-16).

**3.5 BIT timeline (history wrong; 2026-07-13 to 08-14).** The new BIT ("bit traject") objects arrived 2026-05-20 (4ec489c0).
Start and end dates were repaired four times in 11 days: 07-13 (0304e386: the `case` for correctie/korting timestamps),
07-15 twice (080f607f "Fixed startdatum", 138236e2 end date taken from `tijdstipstatusgewijzigd` instead of
`tijdstiplaatstewijziging`), 07-24 (21ee2e34, INTEL-25408), then 08-14 "Timeline fixes for bit objects" (91febbe0), and a
full refactor 10-09 (b9b5a2be). Earlier: 2026-04-17 ee655f5f "onbedoelde deletet objecten".

**3.6 Production tests failing (environment; 2025-10-22/24, INTEL-19814 + INTEL-20707).** "extra logica om foutieve data
in lagere omgevingen op te vangen" (c3c35c2d: 45 lines in `satc_burger_hb`) and two days later "Nakijken DBT testen die
falen in PRD" (05f7cb34): four `not_null` (severity warn) tests on `salc_burger_uuid_ikl_ab` were commented out, a
snapshot cut-off `dv_snapshot_ts >= 2024-09-01` was added to `satc_pit_burger_int_mon_dhb`, a `where burger_uuid is not null`
was added to the privacy sat. Production data had rows the lower environments did not.

**3.7 Per-environment switch-over dates (feed changed at different times; `dbt_project.yml`).** `start_vacature_c2_dt`:
LDV 2025-04-29, ABT 2025-05-13, PRD 2025-05-28, CBT 2025-06-03; `start_correctie_nis_code_2026_dt` PRD 2026-05-27, CBT
2026-06-16; `start_brgdvl_werkloosheidsverzekering_crud_dt` PRD 2026-04-21 vs 2026-04-03 elsewhere. 8 such variables with
LDV/ABT/CBT/PRD values; 10 commits changed them since 2026-05 (pickaxe `-S'"PRD":'`), e.g. 552e93d7 (2026-05-27)
"Updated cutoff date for organisatielabel PRD": 03-24 became 04-08 because the new feed reached PRD two weeks later.

**3.8 Standardisation merged twice, reverted twice, repaired by ticket re-use** (see 2.4): the biggest single "cost" in the
history: 4 big merges/reverts in 9 days (2025-11-24 to 12-03).

**3.9 Refactor ticket INTEL-21190 and the key naming ticket INTEL-21172 (2025-11).** 8 commits in 10 days for "verbetering
FK dummy waarden" and a `dhkey` data type change (8902c018, 11-26), and `hub_sluitendmaatpak_hkey` renamed on 2025-12-05
(ccb8ca6c, INTEL-21172, 25 days after its first commit) with a follow-up ticket INTEL-21619 on 12-08 "pas velden aan in
tabellen na aanpassing in INTEL-21172". A column rename needed a second ticket to fix the tables downstream.

**3.10 The prd branch/tag plumbing (tooling).** 2025-12-08 "(fix) Change prd CI target branch to main" (29c8b0ef); 2026-01-12
"Change pulled tag for cbt/prd deploys" (a042a11f); 2026-08-04 "Hotfix/dbt release dag" (ec8ccd90); 2025-07-28 "apply hotfix
to current ldv version" (ae5f437c, a missing macro `get_first_day_of_current_month`, 12 lines). Each moved a production
deployment rule after it had misfired.

## 4. Repeat offenders

**15 files with the most repair commits** (`topfiles.txt`; counts after dedup, include reverts; the August 2026 folder
move `conform/` to `satc/` splits some histories in two):

| n | file |
|---|---|
| 26 | models/sources/sources_edw_dv_rdv.yml (source declarations: "missing rdv sources" 2026-09-15, "fixed sources for dev" 05-04, reverts) |
| 12 | fagd_ftr_vacature_indicator/t_fagd_ftr_vacature_indicator.sql |
| 10 | fagm_ftr_vacature_indicator/t_fagm_ftr_vacature_indicator.sql |
| 9 | persistentie/t_fagd_ftr_vacature_indicator/t_fagd_ftr_vacature_indicator.sql (older path of the same model) |
| 8 | fagm_ftr_vacature_indicator/t_fagm_ftr_vacature_indicator.yml |
| 8 | satc_burger_indicatie_arbeidsbeperking_hb.sql |
| 8 | models/sources/sources_edw_dv_bdv.yml |
| 6 | fagd/fagm vacature_indicator_pivot .sql (two files, 6 each) |
| 6 | fagm_ftr_vacature_indicator_pivot.yml |
| 6 | dbt_project.yml |
| 6 | satc_burger_indicatie_arbeidsbeperking_hb.yml |
| 6 | satc_vacature_hb_incr.sql |
| 6 | models/sources/sources_edw_inc.yml |
| 6 | refc_niet_inzetbaarheid_reden_definitie_hb.sql |
| 6 | satc_bit_correctie_ab.sql, salc_burger_uuid_ikl_ab.yml |

Reading: three clusters. (a) the vacature-indicator family (3.1), (b) niet-inzetbaarheid (2.3), (c) the source declaration
files, where every new or moved upstream table needs one line and forgetting it breaks a run ("Fixed missing rdv sources").

**Tickets with the most commits** (non-merge, non-revert, dedup subject+date; ids in the subject): INTEL-19625 (8, 2025-08-22
to 09-04, 13 days), INTEL-20557 (8, 2025-10-28 to 11-20, 23 days; 5 review-comment commits on 10-29, then "bug fix" 3 weeks
later), INTEL-21190 (8, 10 days), INTEL-17840 (6, 13 d), INTEL-19270 (6, 7 d), INTEL-21172 (6, 25 d), INTEL-24317 (6, 11 d),
INTEL-19408 (5, 17 d), INTEL-19814 (5, 9 d), INTEL-17846 (4, 6 d).
**Longest span first to last commit (n>=3):** INTEL-24429 52 days (2026-06-05 to 07-27), INTEL-18890 29 d, INTEL-23767 29 d,
INTEL-21172 25 d, INTEL-20557 23 d, INTEL-20000 18 d, INTEL-19408 17 d, INTEL-25676 15 d, INTEL-19246 14 d, INTEL-17840 13 d.
Caveat: only tickets whose id is in the subject; many commits carry no id (689 distinct ids found).

**Branches merged into `dev` most often** (`first-parent dev` merge subjects):

| merges | branch | what it says |
|---|---|---|
| 6 | main (main merged back into dev) | normal housekeeping |
| 5 | feature/INTEL-24469 | 2026-08-13 to 08-19: new incremental `satb_vacature_vacaturecrud_vacature_hb_incr` (2 files, +230) merged 5 times in 7 days, then 4 reverts on 08-19 |
| 5 | feature/INTEL-17433 | 2025-12-08 to 12-16, 5 merges in 8 days (follow-ups after merge) |
| 4 | feature/INTEL-26342 | 2026-09-25 to 09-29, reverted 4 times on 2026-10-01 |
| 4 | INTEL-25900 | 2026-09-11 to 09-28 |
| 4 | INTEL-25249 | 2026-08-12 to 08-18 |
| 4 | INTEL-24777 | 2026-06-10 to 06-22 |
| 4 | INTEL-19085 | 4 merges on one day, 2025-06-27 |
| 3 | improvement/add_airflow_variables, INTEL-27326, INTEL-25524, INTEL-24429 | |

Reading: the same feature branch merged again and again means "merge, notice something, fix on the branch, merge again":
there was no rehearsal before the first merge into `dev`. The pairs 24469 and 26342 end in revert sets: merge-revert
cycles inside a week.

## 5. Production-only and environment differences (examples)

Environments in the history: LDV (local dev), ABT (acceptance), CBT (a pre-production test), PRD (production).
1. **Data volume, memory:** 2026-06-18 470b0d7c OOM "in ABT" for `t_ftr_sollicitatie_activiteit`; 2026-05-06 11b9f499
   "error in CBT"; 2026-09-22 19c58a28 "8u op PRD". Same SQL, different volume: the lower environments were fine.
2. **Production data dirtier than test data:** 2025-10-22 c3c35c2d and 2025-10-24 05f7cb34 (3.6): "tests die falen in PRD".
3. **Switch-over dates per environment:** 3.7 (8 variables x 4 environments, 10 changes).
4. **A column missing in ABT:** 2026-09-29 be992ea0 "t_fas_bit_traject error op ABT": the ordering on `x_bron_upd_dt` was
   removed (commented out of the select and from 4 window clauses); the column apparently did not exist there (inferred).
5. **Environment-specific connection and config:** 2025-05-23 8f4e5ce6 "fix SS host depending on environment" (Airflow
   trigger: the database host naming differs for the ldv/abt environments, and a missing environment variable now raises
   an error); `ai_kow_ikl_filter_dev/stg/prd.yml` (2026-07-31 ca06255f) are separate filter files per environment.
6. **Test severity by environment:** 2026-11-19 ec267008 `set_severity_error_except`: a test is `error` unless the
   current `DBT_DEPLOY_TARGET` is in `warn_envs`; a global `tests_all_warn` var overrides it. The rule "lower environments
   may warn" made explicit after PRD failures.
7. **Deploy plumbing:** 2025-12-08 29c8b0ef (prd CI target branch), 2026-01-12 a042a11f (tag pulled for cbt/prd),
   2025-07-28 ae5f437c (hotfix applied to "current ldv version").

## 6. Full refreshes and rebuilds

Keyword commits (full refresh, herlad, reload, backfill, initial load, herbouw, rebuild, drop/recreate; 15 commits,
dedup): 2025: 0; 26Q1: 1 (d38a9b18 2026-01-15 "initial load 2026_DROP04"); 26Q2: 4; 26Q3: 4; 26Q4: 5. So rebuild
as a topic appears only from 2026 and accelerates from 2026-05.
- **How a rebuild is requested in practice:** not by flipping a variable in `dbt_project.yml`. The five
  `full_refresh_pit_*_db` / `full_refresh_fmc_pit_loading_history` vars are only ever added as `false` and never set to
  `true` in any commit: **0 flip/flop pairs** found (pickaxe `-G'full_refresh[a-z_]*: *true'` on the file; the only
  hits are documentation lines). The recipe lives in `AGENTS.md` / `CONVENTIONS.md` (2026-10-15 58b2a95f, 10-27 479bac8a):
  `dbt build -s fmc_pit_loading_history daily_snapshot_dates pit_xxx_db --full-refresh --vars '{full_refresh_pit_xxx_db: true}'`
  i.e. the flag AND the model-specific variable (double safety), set on the command line at run time.
- Later layers: 2026-05-15 35adb98c "Removed backfill macro to avoid misuse"; 2026-07-31 ca06255f "add full refresh to DAG
  params" (Airflow dag parameter); 2026-09-17 762ddb4f "Prep work for reload DAG"; 2026-10-13 b488180b generic
  `check_full_refresh`; 2026-11-20 6c4e9db5 `should_full_refresh` macro with var `full_refresh_models` ("selectief modellen
  refreshen bij een deploy"); 2026-11-10 7747c6fa chunked/swap full refresh (`full_refresh_strategy: swap`, `chunks`).
- **Signs of cost** (no timings are recorded in git): "8u op PRD" (2.1); OOM at initial load of satc_vacature_hb
  (6019b3cb) so that an "initial loading script" had to be written (aea778aa); 7747c6fa says outright that a full-refresh
  needed the memory of the complete join. Historical reload of data: 2026-05-12 0a2608c3 "Historische data herladen"
  (INTEL-23688, two models, +204/-254).

## 7. Checks added after harm, and checks taken away

**Checks added together with a fix** (diff adds tests in the same commit): 
1. 2026-11-19 ec267008 INTEL-27197: generic `primary_key` test + `set_severity_error_except` (after the duplicate series).
2. 2026-10-23 9bec21f6 INTEL-27136 "Dubbele rijen": yml +90 lines with not_null tests on the repaired data product.
3. 2026-08-05 0c1f8326 "Fixed unique keys test" (unique_combination added on the fagm persistent + presentation tables
   right after the dummy-key and incremental rework).
4. 2026-07-03 4ca8b832 INTEL-25401 "pas unique_combination test aan" (8 lines changed, key of `organisatie_interne_info_ab`).
5. 2026-09-28 f238784f "Duplicates in satc_bit_correctie_ab" (yaml +2, ftr_bit updated with it).
Scan: 48 of 443 non-revert repair commits add a test-like line to a yml (rough; includes brand-new ymls).

**Checks removed, commented out or downgraded** (`tests.py` over `git log -p` of yml files, modified files only,
dedup; indicative): 54 commented-out test blocks, 24 commits that remove >=2 test lines with no additions, 24 that add
`severity: warn` or `enabled: false` without removing a warn: **about 100 commits**. By quarter (all three kinds): 25Q4 23,
26Q1 11, 26Q2 15, 26Q3 32, 26Q4 18. "Commented-out" contains some formatter noise (e.g. b01c0929 2026-09-09 "Formatted yml
files" shows 124 lines).
Examples:
1. 2025-10-13 5fed0d5c "tests satb_burger_teverkennenberoep_hb uitgeschakeld" (not_null + relationships to hubc_burger removed).
2. 2025-10-24 05f7cb34 INTEL-20707: four `not_null` (warn) commented out to get production green (3.6).
3. 2025-10-22 9fb42109 "t_fpsd_burger disabled" and 2025-11-06 b87238a6 "attout disable".
4. 2025-12-23 2acf0968 "intel-19186 burger bemiddeling" adds warn severity, then the whole feature was reverted 2026-01-07.
5. 2026-11-06 20ebbee6 INTEL-27566 "reduce tests run on fact" (`fpsd_burger`, 88 lines of tests deleted for run time).
Also 2025-09-11 a9a2855b "INTEL-19207 test unique id verwijderd". October 2025 is the peak (23 in 25Q4): production
failures were handled by silencing tests, and the structured answer (per-environment severity) came 13 months later.

Not found / not determined: durations of runs other than "8u"; the reason the INTEL-24181 fix was reverted; whether
the 2026-01-07 reverts and e17823b8 reached production.

## Stories worth telling

1. **The eight-hour night (2026-09-22, 19c58a28).** Three small "views" were recalculated every time a big table was built,
   until production needed about eight hours. The fix was 15 lines: save their results as real tables. Nobody had asked the
   system how long it would take until production told them.
   Workshop angle: rehearse before merge on production-sized data; time every model, and a failure mail when a run is late.

2. **A 333-file change merged to production twice, taken back twice (2025-11-24 to 12-03, a0e7ed53, 18dee2ff, 3a52f715).**
   A project-wide tidy-up went straight to the main branch, was reverted the next day, came back a week later and was
   reverted again; only a small ticket-sized fix finally stayed.
   Workshop angle: rehearse before merge; numbered versions and rollback so a revert is one step.

3. **The fix that four releases refused (2026-05-12 to 08-20, a217d8f5, b03c4c6a, 8b51f4d1, cb62b4d8, 68d0a2b6).** A
   corrected history of "not employable" reasons was reverted from releases 13, 14, 18 and 19 by hand, while it stayed in
   others. Each release started from the same shared branch and had to repeat the decision.
   Workshop angle: one team not ready at release time: pick what goes into a release, instead of undoing it every time.

4. **Four features pulled out of one release on one day (2026-01-07, 1adad4a8 and three more).** Release 2026_DROP03 lost
   four tickets that had been merged weeks earlier; one came back six days later.
   Workshop angle: one team not ready at release time; a release is cut from what is ready, not cleaned afterwards.

5. **The date the feed changed differs per environment (dbt_project.yml; PRD 2025-05-28, CBT 2025-06-03; 2026-05-27 552e93d7).**
   A new source format started on four different days in four environments, kept as a table of dates in one file, and one
   had to be moved by two weeks when the feed reached production later.
   Workshop angle: switch-over date for a changing feed; write the date down once per environment.

6. **Out of memory, over and over (satc_vacature_hb, 2026-04-21 to 11-10).** One large table failed for lack of memory in
   April, August and September, got an "initial load" script, an incremental version, and finally (2026-11-10) a rebuild
   in chunks next to the live table. Seven months from first symptom to a structural answer.
   Workshop angle: rebuild without downtime; redo only what failed (chunks and batches).

7. **Same bug, three data products, two weeks (duplicate rows, 2026-10-23 to 11-06; INTEL-27136, 27178, 27187).** Consumers
   got rows twice from the talent, region and time-preference products; only afterwards (2026-11-19, ec267008) one generic
   primary-key test was written that catches the whole family.
   Workshop angle: automatic checks, written once for the class of error, with a stricter level in production.

8. **Silencing the alarm to get a green run (2025-10-13 to 10-24, 5fed0d5c, 05f7cb34).** When tests failed in production
   they were switched off or demoted to warnings; about a hundred such commits exist, with a peak in autumn 2025.
   Workshop angle: automatic checks with an explicit, visible severity per environment; failure mail instead of silence.

9. **A fact table fixed nine times in ten weeks (t_fagd_ftr_vacature_indicator, 2026-08-21 to 10-30).** Missing keys, slowness,
   a rewrite to incremental, doubles, window errors: each repair created the next one.
   Workshop angle: house rules and review before merge; own workbench per developer to test on full-size data.

10. **Thirty-seven attempts at one Docker file (2026-02-27 to 08-06, "Edit dev.Dockerfile").** The developer image was tuned
    by editing the file in the web browser, one pipeline run per guess, one revert on the first day (4a85eeff).
    Workshop angle: own workbench per developer so that tooling is tried locally, not on the shared pipeline.
