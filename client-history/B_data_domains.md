# B. Data domains, layers, sources, switch-over dates, consumers, checks, people (dbt-vdp, branch `dev`, 2026-11-26)

Method in one line: head of `dev` extracted with `g archive` into my scratch folder and counted with grep/python; history by `g log -S`, `--diff-filter=A --name-only` (file adds), `g rev-list -1 --before=<quarter end>`. "Inferred" = my reading, not stated in the repo.
Caveat for all "first appeared" dates: the model folders were restructured on 2025-07-10 (b9b8da94) and models were bulk-migrated from an older flyway/SQL-view world (comments cite "Original flyway file ... INTEL-15040"), so I date a domain by the earliest add of a file whose *name* still exists today.

Size on `dev` over time (`.sql` model files at quarter end): 2025-06-30: 77 | 2025-09-30: 227 | 2025-12-31: 1,005 | 2026-03-31: 1,125 | 2026-06-30: 1,431 | 2026-09-30: 1,736 | 2026-11-26: 1,963. The big jump is Q4 2025 (+778 models in one quarter, mostly the data vault bulk). Commits on `dev`: 3,548; distinct INTEL tickets in subjects: 1,038.

## 1. Layers

Databases (one per layer, `dbt_project.yml`): `edw_dv_bdv`, `edw_ldm`, `im_dimensional`, `pres_dataproduct`, `edw_fmc`, `vdp_mgmt`, `tenant_ai`; project default `+materialized: view`. Raw layer is NOT built by dbt: it is the source `edw_dv_rdv` (raw data vault, 651 tables), landed by another tool ("edw_inc" = incremental/staging mirror of the source systems, 34 tables).
Flow (inferred from `source()`/`ref()` use): source systems -> `edw_inc` / `edw_dv_rdv` (raw vault, outside dbt) -> `edw_dv_bdv` (business data vault, dbt) -> `edw_ldm` (logical data model) and `im_dimensional` (dimensional model) -> `pres_dataproduct` (what consumers read).

Materialization counts = occurrences of `materialized:` in yml/sql per folder (models without a setting are views). Strategy: every incremental model uses `incremental_strategy: delete+insert` ("delete" 47x, `append` 1x, only `vdp_mgmt`). Overall in models: table 473+15, view 782+11, incremental 48, ephemeral 10.

| Folder | Models | What it is | Materialization (counted) | Mark |
|---|---:|---|---|---|
| `edw_dv_bdv/hubc` | 49 | Hubs (business keys, one per business concept: burger, vacature, bit, asb...), built with macro `generate_hubc` | table 45, view 3 | name+macro |
| `edw_dv_bdv/satb` | 212 | Satellites "b": read **directly from raw vault** (`source('edw_dv_rdv')`), add validity `x_geldig_van_dt/tot_dt` | view 84, table 33, incr 1 | inferred b = built on raw ("basis") |
| `edw_dv_bdv/satc` | 255 | Satellites "c": read from `satb`/hubs via `ref()`, combine/conform several sources | table 122, view 79, incr 10 (delete+insert), ephemeral 4 | inferred c = conformed/curated |
| `edw_dv_bdv/lksb` / `lksc` | 132 / 55 | Links between hubs, b-from-raw / c-conformed (e.g. `lksb_ami_burger_oe_oe1_hb`) | view 54 / 16, table 23 / 9 | name |
| `edw_dv_bdv/ldsb` / `ldsc` | 33 / 26 | Link-descriptive satellites ("lds") b / c (validity of a relationship) | mostly view | inferred |
| `edw_dv_bdv/refb` / `refc` | 130 / 138 | Reference data (codes, `refkader` = reference frame), b / c | refb view 29, refc view 67, table 37 | name |
| `edw_dv_bdv/salc` | 7 | Same-as links (id <-> uuid <-> ikl_nr mappings, e.g. `salc_burger_uuid_ikl_ab`) | table 7 | name |
| `edw_dv_bdv/nhlb` / `nhlc` | 4 / 5 | Non-historised links = events/transactions (e.g. `nhlb_brgdvl_vmasverzending`, `nhlc_burger_auditlog`) | view / incr 1 | inferred |
| `edw_dv_bdv/pit` | 12 | Point-in-time tables (daily rows, `_db`), e.g. `pit_burger_db`, `pit_medewerker_db` | table 6, incr 5 | name+yml ("1 rij per ... per dag") |
| `edw_dv_bdv/brg` | 3 | Bridge (e.g. `brg_transmissie_hb`) | incr 1, table 1, view 1 | name |
| `edw_dv_bdv/c2_conversie` | 17 | Occupation-tree conversion process (C1 -> C2) incl. `check_*` gate models that test if inputs (AI predictions, Comeet release) are available | view 15, incr 2 | tag `c2_conversie` |
| `edw_dv_bdv/migr_*` | 2+2+2+1 | One-off migration objects: `migr_vdp1` (old VDP 1.0 archive BIT history), `migr_stratmon` (old monitoring), `migr_dwh` (old DWH), `migr` (`t_vac_tellers_m`) | view/table | folder |
| `edw_dv_bdv/datum` | 2 | date helper dimension | - | name |
| `edw_ldm` | 341 | Logical data model, one folder per business object; each in a pair `x_ab` (current) + `x_hb` (history) | view 266, table 7 | name+yml |
| `im_dimensional/da` | 117 | Dimension, current ("actueel") | table 53, view 16 | yml ("Dimensie ... actueel view") |
| `im_dimensional/dh` | 52 | Dimension, historical | table 25, view 8 | yml ("historisch") |
| `.../ftr` | 59 | Transaction facts | table 27, incr 3 | name |
| `.../fas` | 15 | Accumulating-snapshot facts (inferred; e.g. `fas_bit_traject`, `fas_burger_dienstverlening`) | table 5, view 6 | inferred |
| `.../fpsd` / `fpsm` | 6 / 12 | Periodic-snapshot facts, daily / monthly ("afgesloten maanden"), e.g. burger, arbeidsrelatie, bit_traject | incr 3 / 5 (delete+insert) | yml |
| `.../fagd` / `fagm` | 6 / 10 | Aggregate facts per day / per month (e.g. APIM search history, vacature indicators) | incr 3 / 2 | yml |
| `.../vda`, `vfpsm` | 20, 10 | Views of dimensions/facts feeding indicator reporting (`vda_*_indicatoren`, `vfpsm_indicatoren_*`) | view | inferred |
| `.../brg`, `rpt` | 6, 2 | Dimensional bridges; report views (e.g. `rpt_fas_organisatie_contactmoment`) | table 4 | name |
| `im_indicator_stg` | 5 | Staging views for the indicator ("KPI") reports (`v_st_fact_ind_*`) | view 5 | name |
| `pres_dataproduct/dp_ai` | 81 | Data products for AI/ML teams (read-only roles `dp_im_pres_ai_*`) | view 27, table 10, ephemeral 4 | grants |
| `.../int_dwh` | 34 | "Integratie" views delivering to the (old) DWH, role `im_monitoring_ro` | table 15, view 6 | yml ("Integratie naar dwh") |
| `.../int_mon` | 23 | Integration for monitoring/KPI (burger, beroep, afstand tot arbeidsmarkt) | table 8, view 11 | name |
| `.../dp_burger` 10, `dp_int` 6, `gsb` 9, `int_talentapi` 8, `int_ai` 4, `dp_cv` 4, `dp_jobbeurs` 3, `ai_kow_ikl_filter` 3, `dp_leertijd` 2, `int_vdp` 2, `dp_opl`, `dp_fin`, `dp_gebruiker`, `workshops`, `courses`... | | see section 5 | | |
| `edw_fmc` | 2 | Framework/metadata (`fmc_pit_loading_history`, `full_refresh_fmc_pit_loading_history` var) | view | name |
| `vdp_mgmt` | 11 | Platform self-monitoring: `dbt_models`, `dbt_run_results`, `dbt_table_grants`, `dbt_orphaned_objects`, `dbt_active_test_failures`, `singlestore_table_info`... | table 6, incr 1 | name |
| `tenant_ai` | 1 | `vdp_dbt_run_results` for an AI "tenant" (added 2026-09-22, c2ac4e79, INTEL-26469) | view | commit |

Naming suffixes (decoded from descriptions, inferred where marked):
- `_hb` = historiek (history, rows with `x_geldig_van_dt / x_geldig_tot_dt` validity): 830 in dv_bdv, 155 ldm, 109 pres. `_ab` = actuele (current state; yml: "AB-versie", "Actuele versie _ab"): 97 dv_bdv, 153 ldm, 43 pres. They come as pairs.
- `_hf` = functional history (a "functionele tijdslijn", e.g. teams/clusters, per AGO-period): 34 dv_bdv, 3 ldm, 3 pres (inferred).
- `_db` = day-level (one row per day, PIT) 9; `_dhb` = daily history 9+5; `_mhb` / `_mab` = monthly history / month-start snapshot ("ab van int_mon_burger_hb waarbij de situatie op eerste dag ... van de huidige maand telt"); `_dab` daily ab; `_af` (3, inferred "actueel/afgeleid", e.g. `int_hc_workshop_e_learning_af`, `dp_fin_ovo_hierarchie_af`).
- `_act`, `_scd2`, `_scd2c`, `_scd2_pers` (ldm, 9/5/5/5): current / slowly-changing history / with correction / persisted; tagged `boin` (28 models; inferred: a Business-Intelligence project label).
- `_incr` = incremental twin of a table model (e.g. `satc_burger_hb_incr`; its README: "draait parallel naast de gewone ... proef", a trial run next to the old version).
- Prefix `t_` = materialised table copy of a view (e.g. `t_int_dwh_klant_ab`).

## 2. Business domains (decoded from names; counts = model files whose name matches)

| Domain | Gloss | dv_bdv | ldm | im_dim | pres | First on `dev` |
|---|---|---:|---:|---:|---:|---|
| burger | citizen = jobseeker (also werkzoekende, kandidaat) | 211 | 112 | 38 | 89 | 2025-03 |
| vacature | job vacancy (also `vac_`, vacaturecrud) | 118 | 13 | 49 | 23 | 2025-04 |
| beroep + conversie | occupation; C1->C2 occupation-tree conversion | 102 | 34 | 5 | 42 | 2025-07 |
| competent/competentie/skill | competence, soft skills, competence reports | 113 | 43 | 2 | 14 | 2025-05 |
| organisatie/onderneming/werkgever (+graydon) | employer organisation; Graydon = external company data supplier | 102 | 41 | 34 | 3 | 2025-08 |
| ibo | individual occupational training (contract with employer) | 70 | 8 | 24 | 2 | 2025-10 |
| c2 / comeet | new occupation standard C2; Comeet = occupation matching tool | 105 | 14 | 4 | 31 | 2025-07 |
| bit | benefit-related track/status object (`bit_status`, `bit_traject`; inferred; migrated from VDP 1.0) | 22 | 6 | 32 | 0 | 2026-04 |
| refkader | reference-code frame (gemeente, land, studie, codes) | 76 | 0 | 0 | 0 | 2025-04 |
| interne organisatie / medewerker / arbeidsrelatie | VDAB's own staff, teams, clusters | 52 | 9 | 34 | 3 | 2026-02 |
| opleiding / cursus / stage / studie / workshop | training, courses (Totara learning platform, "tenant_leertijd" apprenticeship) | 108 | 51 | 1 | 34 | 2025-03 |
| sollicitatie / gesprek / jobbeurs | job application, interview, job fair | 59 | 2 | 15 | 12 | 2025-04 |
| controledossier | benefit-control (compliance) case file | 20 | 8 | 0 | 0 | 2026-07 |
| transmissie | transmission of case data to other bodies (extra decision, proof document) | 21 | 6 | 0 | 0 | 2025-10 (ldm) / BDV 2026-10-27 |
| afsprakenbundel (asb) | bundle of agreements (service per jobseeker) | 15 | 10 | 0 | 8 | 2026-09 |
| dienstverlening / sluitend maatpakket | service delivery, "tailor-made package" | 53 | 12 | 26 | 3 | 2025-09 |
| arbeidsbeperking / indicatie / vaph | work disability indication, VAPH aids | 23 | 16 | 0 | 1 | 2025-10 |
| beschikbaarheid / niet-inzetbaarheid | availability / unavailability of jobseeker | 37 | 10 | 1 | 2 | 2025-04 |
| indicatoren | KPI/indicator reporting | 3 | 0 | 33 | 4 | 2025-10 |
| apim | API-manager usage (search history, profile requests) | 0 | 0 | 22 | 0 | 2026-05 |
| gebruiker / login / auditlog | users, logins, audit trail | 58 | 0 | 5 | 4 | 2025-04 |
| crm | customer contact (contactpersoon, activities) | 7 | 0 | 3 | 0 | 2025-09 |
| ai / talentapi | outputs for AI and Talent API | 2 | 0 | 0 | 94 | 2025-03 |
| dwh integratie | delivery to old DWH | 2 | 0 | 0 | 34 | 2025-08 |
| monitoring (int_mon, stratmon) | monitoring/KPI feeds | 9 | 0 | 0 | 31 | 2025-05 |
Domains overlap (a model can match two rows), so the columns do not sum to 1,963.

Timeline by quarter (file adds of matching names that still exist; counts):
- 2025 Q1: burger 4, ai 3, platform/mgmt 9. The very first real data models; `python/generate_sources.py` generated the sources.
- 2025 Q2: vacature 13, burger 23, ai/talentapi 13, gebruiker 4, sollicitatie 10, beroep start (int_talentapi 2025-05-19, kow switch, section 4).
- 2025 Q3: beroep 33, c2/conversie 19, organisatie 27, burger 41, competent 8, dienstverlening 1 (the C2 occupation-tree programme, INTEL-19481/19484, Aug 2025).
- 2025 Q4 (the bulk): burger 209, competent/skill 82, refkader 55, opleiding/cursus 116, beroep 47, vacature 25, ibo 11, dwh 15, indicatoren 19, beschikbaarheid 26, dienstverlening 22, ai 30. This is the main data-vault + LDM load.
- 2026 Q1: burger 55, beroep 32, opleiding 19, organisatie 19, c2 25, vacature 17, interne org 4.
- 2026 Q2: vacature 77, organisatie 55, ibo 36, bit 42, interne org 19, apim 10, opleiding 18, competent 20, indicatoren 7.
- 2026 Q3: interne org 53, afsprakenbundel 24, controledossier 26, ibo 34, organisatie 32, c2 34, competent 32, dienstverlening 23, burger 37, apim 8.
- 2026 Q4 (to 11-26): transmissie 26 (BDV, INTEL-25568, 2026-10-27), arbeidsbeperking/vaph 29, sollicitatie/jobbeurs 26, c2 24, dienstverlening 25, burger 39, opleiding 25.
Model counts per layer at quarter end: dv_bdv 130 (Q3-25) -> 589 -> 639 -> 786 -> 965 -> 1,089; ldm 14 -> 191 -> 243 -> 268 -> 306 -> 341; im_dim 39 -> 89 -> 102 -> 219 -> 281 -> 315; pres 44 -> 127 -> 126 -> 141 -> 165 -> 199. im_dimensional doubled in Q2 2026.
Commit-subject work-stream labels (top): [INT_MON_BURGER] 23, [SAS historiek ...] 26 in total, [Burger - LDM] 12, [C2 HOOFDPROCES IBO/VAC] 10, [refkader] 7, [Menselijk Kapitaal - Dim Model Q3] 6, [Competentierapporten] 6, [Beroepenboom] 5, [Burger - Auditlog] 4.

## 3. Sources (`models/sources/`, 14 files)

Source files were not under version control until 2025-11-20 (8fc858e4 "Added currently used sources to version control"); before that they were generated by `python/generate_sources.py` (visible 2025-03/06).

| File (database) | Stands for | Tables now | First committed | Growth (tables at quarter end) |
|---|---|---:|---|---|
| `sources_edw_dv_rdv` | raw data vault: 264 `sat_`, 186 `lks_`, 122 `ref_`, 42 `hub_`, 31 `lds_`, 4 `nhl_`, 2 `lnk_` | 651 | 2025-03-11 (generated), in git 2025-11-20 | Q4-25 334, Q1-26 355, Q2 428, Q3 578, now 651 (source() refs used in models: 7 at 2025-03, 26, 62, 323, 354, 388, now 553) |
| `sources_edw_inc` | staging mirror of source systems (ami_*, brgdvl_indicatie_arbeidsbeperking_*, comeetng_*, orgbeheer_*...) | 34 | 2025-11-20 | 6, 13, 16, 28, 34 |
| `sources_edw_mtd` | metadata of the loader (load cycles, loading windows, exception records) | 95 | 2025-11-20 | n/a |
| `sources_edw_dv_bdv` | tables of the business vault read back by name | 8 | 2025-10-09 (first), edw_dv_bdv 2025-04 | 3,2,3,4,8 |
| `sources_edw_fmc` | framework metadata | 1 | 2025-11-20 | |
| `sources_vdp_mgmt` | platform parameters/monitoring (`vdp_parameters`, run results) | 2 | 2025-11-20 | |
| `sources_information_schema` | database catalog (used by `vdp_mgmt`) | 6 | 2025-11-18 | |
| `sources_vdp_psa` | frozen VDP 1.0 / DWH history (`bit_status_vdp1_historiek`, `bit_periodestatus_vdp1_historiek`, `dwh_hist_ibo_*`, `dwh_t_vac_tellers_m`, c2 conversion code lists) | 7 | 2026-03-12 | 2 (Q1) -> 5 (Q3) -> 7 |
| `sources_tenant_leertijd` | apprenticeship ("leertijd") tenant: inschrijvingen, examens, trajecten, centra... | 13 | 2026-03-27 | 7 -> 13 |
| `sources_tenant_ai` | AI tenant | 1 | 2026-01-30 | |
| `sources_im_dimensional` | dimensional layer used as a source by presentation views | 22 | 2026-05-29 | 22-23 |
| `sources_tenant_monitoring_historiek` | old SAS monitoring history (`sas_hist_ibo_logins_start`, `sashist...int_mon_burger_hb`) | 3 | 2026-06-19 | |
| `sources_ondersteuning_inc_l` | support tooling (`apim_zoekresource`) | 1 | 2026-09-25 | |
| `sources_psa` | empty placeholder (`tables: []`) | 0 | 2026-10-30 | |

- `freshness` / `loaded_at_field`: 0 occurrences in all source files. Not found: the project never uses dbt source freshness (loading is tracked by `dv_load_ts`, `kafka_timestamp`, `edw_mtd` load-cycle tables instead).
- Source-system words in the 651 raw-vault tables (second name part): brgdvl 181 (burger dienstverlening, inferred), refkader 71, ami 69 (legacy application "AMI"), burger 54, vacature 34, orgbeheer 29, interneorg 24, comeetng 23, totara 17, gebruikersbeheer 13, smp 11, kyss 11, controledossier 10, competentieversterking 9, sollicitaties 7, ksz 6 (inferred: federal social-security data), graydon 6.
- Event streams: Kafka is everywhere. 181 model files select `kafka_timestamp`, 257 files mention kafka (columns `x_kafka_topic`, `crud_kafka_timestamp`, `x_crud_kafka_topic`). 124 files mention "crud". "CRUD" topics are the new event topics (e.g. `sat_vacature_vacaturecrud_*`, `sat_orgbeheer_organisatiecrud_label`; 19 rdv tables have crud in the name) versus older "summary"/plain topics (`sat_brgdvl_werkloosheidsverzekering_summary`). Hb/ab: the `_hb` models are the historised view of such a stream, `_ab` the current state.
- Legacy feeds: `migr_vdp1` (VDP 1.0 archive), `migr_stratmon`, `migr_dwh`, `vdp_psa`, `tenant_monitoring_historiek` (old SAS history). 4 models run both an old and a new path by date (section 4). Files mentioning "sas": 10 (bare word). `dwh_flow_*` models existed and were deleted 2025-06-26 (14d11e0e).

## 4. Feeds that changed: switch-over / cut-off variables

Mechanism: `dbt_project.yml` vars with one date per environment, picked in SQL by `var("x")[env_var("DBT_DEPLOY_TARGET","LDV")]`. Keys (inferred): LDV = development/first environment, ABT = acceptance, CBT = a later pre-production stage, PRD = production. Pattern in SQL: old source `where kafka_timestamp / x_geldig_van_dt < cutoff`, `union all` new source `>= cutoff`, so history from the old feed and the future from the new feed fit together. The dates are exact time stamps (hh:mm:ss) of the moment each environment's new feed started, so each environment has its own date because the producer/topic or the fix was switched per environment on a different day. Most vars were committed after the dates they contain (retro-fitted), only the last one is a future date. 66 commits touched `dbt_project.yml`.

| Var | Added (hash, date) | Ticket | Used by (models) | Dates LDV / ABT / CBT / PRD |
|---|---|---|---|---|
| `kow_switch_date` (global, `now()` based; removed) | 9b6bc8ee 2025-04-24; changed 9bdca4e8 2025-06-12; removed 14d11e0e 2025-06-26 | (not in subject; "vdp flow voor beroepen") | `dp_ai_burger_gewenst_beroep_hb`, `dp_ai_burger_talenkennis_hb` (AI "kans op werk" data product) | 2025-06-15, then 06-14 |
| `start_vacature_c2_dt` | 27e8d978 2026-11-23 | INTEL-27758 | `satb_vacature_beroep_summary_and_crud_hb` | 2025-04-29 15:15 / 2025-05-13 16:14 / 2025-06-03 13:00 / 2025-05-28 16:52 |
| `start_organisatie_label_v3_2026_dt` | 3d843381 2026-05-04; PRD corrected 7970cf1c 2026-05-27 | INTEL-23700 | `satc_organisatie_label_hb` | 2026-03-25 / 03-25 / 03-24 / PRD 03-24 -> **04-08** |
| `bit_cutoff_date_2026_dt` | 4ec489c0 2026-05-20 | INTEL-23866 (history load INTEL-23047) | 9 files: `satb_brgdvl_bit_hb`, `satb_brgdvl_bitperiode_hb`, `satc_bit_status_hb`, `satc_bit_correctie_ab`, `satc_bit_korting_ab`, `satc_bit_gelijkstelling_ab`, `ldsc_bit_periode_hb`, `migr_vdp1_bit_hb` | 2026-03-09 08:13 / 03-10 08:43 / 03-19 12:46 / 03-23 19:54 |
| `start_correctie_nis_code_2026_dt` | 34df7ee3 2026-07-02 (merge cd3d901a) | INTEL-24569 (branch) | `refb_refkader_code_gemeente_nis_hb` | 2026-05-28 09:49 / 05-28 09:40 / 06-16 08:30 / 05-27 10:34 |
| `start_correctie_landmon_origine_2026_dt` | 384426b9 2026-07-20 | not in subject | `refb_refkader_codes_landmon_hb` (countries, origin) | 2026-06-12 10:47 / 06-11 20:06 / 07-14 09:20 / 06-11 20:01 |
| `burger_switch_sas_vdp_dt` (one date, all envs) | 7f2df391 2026-07-28 | INTEL-20932 (SAS historiek) | `dp_int_mon_burger_hb`, `t_dp_int_mon_bg_alt_beroep_hb`, `t_dp_int_mon_burger_migratie_mhb`, `t_dp_int_mon_burger_beroep_migratie_mhb` | 2025-07-01 |
| `burger_afstand_switch_sas_vdp_dt` | 57a7509f 2026-08-10 | INTEL-24159 | `t_dp_int_mon_burger_migratie_mhb`, `int_mon_burger_afstand_dhb` | 2024-10-05 |
| `start_brgdvl_werkloosheidsverzekering_crud_dt` | b56f8ebf 2026-08-28 | "BWIT" (benefit, unemployment insurance) | `satb_brgdvl_werkloosheidsverzekering_summary_hb` | 2026-04-03 x3 / PRD 2026-04-21 |
| `start_brgdvl_indicatie_arbeidsbeperking_crud_dt` | 11e8b069 2026-10-22 | INTEL-26840 | `pit_burger_indicatie_arbeidsbeperking_db` | 2026-11-17 in all four (future date at commit time) |
Also a parameter table instead of a var: `vdp_parameters` key `satb_vacature_vac_vacature_hb_cutoff_dt` read by `satc_vacature_hb` and `satc_vacature_hb_cte_date_range` (first c809a916 2026-04-02; "cutoff date and memory issues fix" fd2813f8 2026-04-21; incremental version 864d1963 2026-07-28 INTEL-25107, fix 3c2030f5 2026-10-09). Cut-off between old vacature topic `vac` and new `vacaturecrud`.

Plain-language stories (2 sentences each):
- vacature c2 (`start_vacature_c2_dt`): vacancy occupations used to come in a vacancy topic; from a per-environment moment in Apr-Jun 2025 they arrive with the new C2 standard in the CRUD topic. Dates differ per environment (LDV earliest 2025-04-29, CBT latest 2025-06-03), and the var only entered git in Nov 2026, 17 months after the real switch.
- organisatie label (INTEL-23700): the employer-label data moved from its own topic to a v3 organisation topic with finer granularity (organisation + label uuid) and a new "notities" field; the model shows V2 rows before the date and V3 after (`'V2' as bron` / `'V3'`). Production went live later than planned: PRD date edited from 2026-03-24 to 2026-04-08 on 2026-05-27.
- BIT: the new BIT business vault starts at go-live (March 2026, per environment) while the old VDP 1.0 archive supplies history. A comment in `satb_brgdvl_bit_hb` records a gap: the archives stop on 2025-08-05, go-live is 2026-03, "wat daartussen ligt valt weg": 110,568 raw-vault rows over 52,355 BITs would be lost; the fix is to put the cut-off at 2025-08-05 (the var is shared with five other models).
- nis_code / landmon corrections: reference data (municipality NIS codes, country origin) were corrected in the source from a moment per environment; the models keep rows whose validity ends after that date so the correction is carried through the whole history. Environments deviate by weeks (CBT 06-16 versus 05-27 in PRD for nis_code; 07-14 versus 06-11 for landmon).
- SAS -> VDP (`burger_switch_sas_vdp_dt` 2025-07-01, `burger_afstand_...` 2024-10-05): before the date the monitoring figures per burger/occupation come from the delivered history of the old SAS system, after it from VDP; models compute `x_burger_cutoff_dt` per row (`least(valid_to, switch)`) and the integration view cuts at it. One date for all environments because the history is a frozen delivery, not a live feed.
- werkloosheidsverzekering and indicatie arbeidsbeperking: the burger-dossier events moved from a "summary" topic to a "crud" topic; history comes from the old one up to the date. The second date is in the future when committed (planned switch 2026-11-17), so it works as a planned switch.
- Early version: 2025 `kow_switch_date` used `now()` compared with a date (not a per-environment date), moved a day on 2025-06-12 and was removed with the old `dwh_flow_dp_ai_burger_hb` on 2025-06-26 (-293/+134 lines).

## 5. Consumers and data products

| Folder (`pres_dataproduct/`) | Models | Consumer (inferred from names/grants) | First (median add) |
|---|---:|---|---|
| `dp_ai` | 81 | AI/ML teams: roles `jobmatch` (51 refs), `vacaturedata` (39), `kow` (30, kans op werk), `profielmap` (18), `jobmap` (15), `koh` (13), `beroepenzkr` (11, occupation search), `talentapi` (7) | 2025-03 (median 2025-11) |
| `int_dwh` | 34 | old DWH ("Integratie naar dwh"), role `im_monitoring_ro` | 2025-08-26 |
| `int_mon` | 23 | monitoring/KPI | 2025-05-09 |
| `dp_burger`, `gsb`, `dp_int` | 10, 9, 6 | citizen-facing data / documents / integrated monitoring | 2025-09-02, 2025-10-28, 2026-07-28 |
| `int_talentapi` | 8 | Talent API | 2025-05-19 |
| `int_ai`, `ai_kow_ikl_filter` | 4, 3 | AI; "kow IKL filter" dev/prd tables (INTEL Dimona, 93ad1754 2026-06-30), only AI roles get insert/update (3 each) | 2025-07-23, 2026-06-30 |
| `dp_cv`, `dp_leertijd`, `dp_opl`, `dp_jobbeurs`, `dp_gebruiker`, `dp_fin`, `int_vdp` | 4,2,1,3,1,1,2 | CV/courses (tagged boin), apprenticeship, training reimbursement, job fair, users, finance (role `dp_im_pres_finance_ro`), C2 conversion loop-back to the data lake (role `...competent_conversie_ro`) | 2025-10 .. 2026-11 |
| `tenant_ai` | 1 | AI tenant run results | 2026-09-22 |
Other read roles by count of grant mentions: `im_monitoring_ro` 456, `im_planning_ro` 420 (HR planning), `dp_im_dim_bit_ro` 28, `dp_edw_ldm_bit_ro` 6, `dp_im_pres_mon_burger_audit_ro` 2, a planning audit role 1.
- `grants:` key: 608 occurrences (first 3c533b96 2026-02-13). `contract:` 48 (`enforced: true` 46, 1 false), first `enforced: true` 010abf3b 2026-06-05 (INTEL-23238); `constraints:` 9; `group:` 111 (c2_conversie 42, dim_burger 27, interne_organisatie 18, dim_organisatie 11, kans_op_werk 6, dim_beroep 6, indicatoren 1; 7 groups in `groups.yml`, each with an owner e-mail, not listed here).
- `exposures`: 0 files. `meta:` keys in model yml: 0 (no owner/team/domain/collibra meta); `meta` only appears in singular tests (Test/Doel/Creatie). Tags: "boin" 28 models (LDM `act/scd2/scd2c/scd2_pers` of burger, cursus, opleiding...), "on-demand" 4 (the `migr_*_mhb` monthly migration models and `dp_burger_competentiebewijs_foutenrapport`), "crm" 2, "burger" 1, "forced_refresh" 1, "monthly" 0 in models (only named in `selectors.yml`). Folder tags: layer name tags in `dbt_project.yml`.
- `selectors.yml` (added 762ddb4f 2026-09-17, fixed 432b08f9 2026-10-19): `critical_tests` (= unique_combination_of_columns), `non_critical_tests`, `daily_load` (default: all models and seeds, excluding tag `on-demand` and non-critical tests, "indirect_selection: buildable").
- `dbt-bouncer.yml` house rules since 2025-03-28 (pre-commit b26e6420): no trailing missing semicolon, no hard-coded references, forbids `to_char(`/`str_to_date(`.

## 6. Checks on the data

Data tests in yml at head (`dev`): **5,015** across 1,717 model yml entries. By type: `not_null` 3,243; `dbt_utils.unique_combination_of_columns` 620; `relationships` 609; `accepted_values` 375; `unique` 147; `dbt_utils.expression_is_true` 21. Per layer: dv_bdv 3,062; im_dimensional 1,128; edw_ldm 512; pres 313.
Severity: project default `+severity: warn`. Explicit `error`: 1,592; explicit `warn`: 356; unset (= warn): 3,067. So about 32% of tests can fail a run; the rest only warn.
Unit tests: 0. Singular tests in `tests/`: 19 files (`assert_*`): they reconcile a model to its staging source in `edw_inc` ("vergelijking met staging", header gives creator and ticket, e.g. `assert_organisatie_ab` INTEL-15908, 2025-04-24) with `severity="warn"`, covering organisatie (5), burger indicatie arbeidsbeperking/uitkering/categorie, onderwijskwalificatie, apim (fagd/ftr), werkgeverslead, lokaal klantencentrum. Also `tests/generic/primary_key.sql` (added ec267008 2026-11-19, INTEL-27197: fails on duplicates or nulls in the key; severity "error" except in environments listed in `warn_envs`, macro `set_severity_error_except`) and `tests/integration` (k8s test). The new `primary_key` test has 0 uses in yml yet; 319 models already declare `config.primary_key`, so it is ready to be wired in (inferred).

Growth per quarter (last `dev` commit of the quarter; yml parse):

| Quarter end | Models with yml | Data tests | not_null | unique_comb | relationships | accepted | unique | severity error / warn / unset | Singular tests |
|---|---:|---:|---:|---:|---:|---:|---:|---|---:|
| 2025-03-31 | 5 | 2 | 0 | 0 | 1 | 0 | 1 | 0/0/2 | 0 |
| 2025-06-30 | 22 | 14 | 3 | 9 | 1 | 0 | 1 | 0/0/14 | 0 |
| 2025-09-30 | 106 | 160 | 93 | 44 | 11 | 1 | 11 | 0/0/160 | 4 |
| 2025-12-31 | 381 | 725 | 435 | 99 | 135 | 33 | 23 | 0/120/605 | 4 |
| 2026-03-31 | 873 | 1,210 | 686 | 158 | 252 | 89 | 25 | 0/146/1,064 | 8 |
| 2026-06-30 | 1,167 | 2,017 | 1,131 | 279 | 340 | 172 | 81 | 96/192/1,729 | 12 |
| 2026-09-30 | 1,487 | 3,653 | 2,246 | 455 | 496 | 301 | 134 | 598/295/2,760 | 17 |
| 2026-11-26 | 1,717 | 5,015 | 3,243 | 620 | 609 | 375 | 147 | 1,592/356/3,067 | 19 |
The `error` severity first shows up 2026-05-20 (a737db68, "vervang unique key door unique test") and 2026-06-01 (85bfcee6); it then comes in with new model batches: 109 `severity: error` lines in one commit 28d296cf (2026-08-19, INTEL-24897 teams/clusters), 93 in 541600fa (2026-11-19, IBO), 75 in 5d39d744 (2026-10-26, INTEL-26837 vaph aids). Tests grew 2.5x between 2026-03 and 2026-11, faster than models (1.6x).

## 7. Who works where (numbers only)

Distinct commit authors (non-merge commits touching the path, identity = e-mail id; 24 distinct ids overall, 48 distinct display names, so display names are inconsistent and the true number is the lower one):

| Quarter | Commits | Authors total | dv_bdv | ldm | im_dim | pres | sources | macros | airflow |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 2025 Q1 | 11 | 3 | - | - | - | - | 2 | 1 | 1 |
| 2025 Q2 | 184 | 6 | (before folder restructure: models under another path, not split) | | | | 5 | 4 | 4 |
| 2025 Q3 | 264 | 14 | 14 | 7 | 6 | 10 | 0 | 5 | 3 |
| 2025 Q4 | 454 | 17 | 16 | 7 | 12 | 15 | 8 | 6 | 4 |
| 2026 Q1 | 258 | 19 | 19 | 14 | 10 | 14 | 15 | 7 | 1 |
| 2026 Q2 | 247 | 19 | 18 | 11 | 11 | 11 | 14 | 5 | 3 |
| 2026 Q3 | 274 | 15 | 15 | 12 | 10 | 14 | 13 | 4 | 3 |
| 2026 Q4 (partial) | 187 | 17 | 15 | 9 | 8 | 13 | 11 | 5 | 3 |
`vdp_mgmt` 1-2 authors, `tenant_ai` 1. Numbers are upper bounds for a "team" because one person may use several identities.
Separate teams per layer or domain: **mostly not found.** Almost every author touches `edw_dv_bdv` (15 to 19 of 19) and `pres_dataproduct`, so people work across layers by *ticket*, not by layer. Not found: CODEOWNERS, `meta.owner`, any team in branch names (63 `feature/INTEL-...`, 14 `release/...`, 5 `bugfix`, plus a few personal staging branches). Found weak signals of domain ownership: `groups.yml` has 7 groups with one named owner each (indicatoren, kans_op_werk, dim_burger, dim_beroep, dim_organisatie, c2_conversie, interne_organisatie), 111 models carry a `group`; subject tags in commit subjects such as "[Menselijk Kapitaal - Team Planning]" (a project team label) and "[Burger - LDM]". Inferred: one shared team of about 15 to 19 active developers organised in work streams (programmes) by ticket.

## Stories worth telling

1. **A feed changes on a different day in every environment (2026-05-04, 3d843381; PRD fix 7970cf1c 2026-05-27).** The employer-label data moved from one source to a newer version of it. Test, acceptance and a pre-production stage switched on 24-25 March 2026, but production only on 8 April, and the date written for production in the project was wrong for seven weeks until someone corrected it. The model shows both versions side by side and cuts at the date per environment.
   Workshop angle: switch-over date for a changing feed, one date per environment, and rehearse in the same order as production.

2. **The gap between two systems nobody saw (2026-05-20, 4ec489c0; comment in `satb_brgdvl_bit_hb`).** For the new BIT data the team loaded the old archive up to the go-live date, but the archive had stopped on 5 August 2025 while go-live was in March 2026. A comment in the code says 110,568 records on 52,355 BITs fall in between and would silently vanish; the fix is to move the cut-off back so both sources join without a hole. Five other models share the same date.
   Workshop angle: switch-over date for a changing feed, plus automatic checks that count rows on both sides of the join.

3. **A date set into the future, and dates written long after (2026-10-22, 11e8b069; 2026-11-23, 27e8d978).** The work-disability indication feed has a planned switch on 17 November 2026, identical in all four environments, committed weeks before. The vacancy occupation switch of spring 2025 only got its variable in November 2026, nearly 18 months late, with a different date per environment (29 April to 3 June 2025).
   Workshop angle: switch-over date for a changing feed, recorded when it is decided, not afterwards; house rules for where such dates live.

4. **A temporary switch that lived two months and took 290 lines with it (2025-04-24 to 2025-06-26, 9b6bc8ee, 9bdca4e8, 14d11e0e).** The AI "kans op werk" data product flipped from the old data-warehouse flow to the new platform flow at a date in June 2025, using "now()" in the query. The date moved a day on 12 June, and after the switch a clean-up commit deleted the old flow completely.
   Workshop angle: numbered versions and rollback; remove old paths once the switch is proven.

5. **Checks grew from 2 to 5,015, but two out of three only warn (2025-03-31 to 2026-11-26).** The project default is that a failing check gives a warning, and only 1,592 of 5,015 are set to stop the run. The first "error" severity appears on 2026-05-20; on 2026-11-19 (ec267008, INTEL-27197) a new primary-key check was added that is strict everywhere except in listed environments.
   Workshop angle: automatic checks, and what a failure should block.

6. **Compare against the source, by hand-written reports (2025-04-24, `assert_organisatie_ab`, INTEL-15908).** 19 singular tests recompute a result straight from the staging tables and compare it with the finished model, each with a header with purpose, author and ticket. They all stop at "warn".
   Workshop angle: automatic checks; reconcile against the source.

7. **A table that took too much memory (2026-04-21, fd2813f8; 2026-07-28, 864d1963; 2026-10-09, 3c2030f5).** The vacancy history model needed a cut-off between the old and the new vacancy topic, hit memory problems, was rebuilt incrementally in July and fixed again in October. A parallel "_incr" copy of the citizen history ran next to the old model with the instruction "if you change one, change both".
   Workshop angle: rebuild without downtime, run old and new side by side, redo only what failed.

8. **Corrections that must reach the whole history (2026-07-02, 34df7ee3; 2026-07-20, 384426b9).** Corrected municipality codes (NIS) and country-origin codes were fixed in the source at a different moment per environment (production 27 May, pre-production 16 June). The models take the corrected rows and keep their validity back through history, so reports for past years change too.
   Workshop angle: documentation of what changed on which date; rehearse before merge.

9. **Old figures from the previous system, frozen at a date (2026-07-28, 7f2df391; 2026-08-10, 57a7509f).** Monitoring numbers per jobseeker come from a delivered history of the old SAS system until 1 July 2025 (and until 5 October 2024 for distance to the labour market) and from the new platform afterwards. The cut is computed per row, so a period that straddles the date is shortened at the switch.
   Workshop angle: switch-over date for a changing feed; one frozen source, one live source.

10. **One shared team, 1,963 models, no owner fields (2026-11-26).** Fifteen to nineteen developers touch the vault layer every quarter, there is no CODEOWNERS file and no `meta` owner on any model, and seven "groups" with a named owner cover only 111 models. The quarter of the big load (Q4 2025) added 778 models in three months.
    Workshop angle: house rules / formatter and documentation; who owns what.
