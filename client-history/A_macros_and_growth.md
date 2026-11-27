# A. How the macros and the project grew (dbt-vdp, branch `dev`)

Method: `g log --first-parent dev` (last commit of each month), `g ls-tree -r --name-only <c>`, `g grep -c` for macro definitions, `g log -M --name-status -- macros` for the inventory, `g show <c>:file` for old versions. Commit subjects are often Dutch (glossed in English). No names, hosts or URLs are included.

Caveats: (a) "commits touching" in section 2 counts commits on all branches, merge commits included, so it is inflated by merges (an `odi-dev` import shows up twice, as 9ad40c9a and as the first-parent merge 4ce61ff4). (b) "macro lines" counts every file in `macros/`, yml documentation included. (c) The sections on the early history (Feb to Sep 2025) rest on thin commit messages ("macros", "commit", "squash").

## 1. Growth table (last first-parent commit of each month)

Model .sql files per top-level folder under `models/`. "Other" = small folders: dbt_swf (the very first layout), edw_fmc, im_indicator_stg, meta, tenant_planning, tenant_ai, vdp_mgmt. "Mac files" includes the macro yml docs; "defs" = count of `{% macro` / `{%- macro`; "lines" = all lines in `macros/`. Singular tests = `tests/assert_*.sql`; generic = `tests/generic/*.sql`; seeds = `seeds/**/*.csv`.

| Month | commit | edw_dv_bdv | edw_ldm | im_dimensional | pres_dataproduct | other | models yml | mac files | macro defs | mac lines | singular | generic | seeds |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 2025-02 | 6db083c3 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| 2025-03 | f2adb1f9 | 0 | 0 | 0 | 0 | 0 | 1 | 2 | 2 | 20 | 0 | 0 | 0 |
| 2025-04 | 36a2a0b1 | 0 | 0 | 0 | 0 | 0 | 1 | 4 | 4 | 49 | 0 | 0 | 0 |
| 2025-05 | 3010eb5a | 0 | 0 | 0 | 0 | 44 | 13 | 13 | 13 | 229 | 0 | 0 | 0 |
| 2025-06 | dde222a2 | 0 | 0 | 0 | 0 | 77 | 23 | 16 | 16 | 424 | 0 | 0 | 0 |
| 2025-07 | def7fa99 | 51 | 1 | 8 | 33 | 0 | 24 | 24 | 23 | 790 | 0 | 0 | 0 |
| 2025-08 | 528a3ea2 | 111 | 16 | 27 | 38 | 0 | 55 | 24 | 24 | 833 | 5 | 0 | 0 |
| 2025-09 | 2cc985ba | 135 | 18 | 39 | 44 | 0 | 112 | 24 | 24 | 833 | 4 | 0 | 0 |
| 2025-10 | 201137b0 | 486 | 40 | 49 | 81 | 1 | 204 | 42 | 38 | 1,197 | 4 | 0 | 0 |
| 2025-11 | b1a64ce5 | 544 | 47 | 83 | 120 | 8 | 306 | 44 | 43 | 1,394 | 4 | 0 | 2 |
| 2025-12 | dd88a910 | 589 | 191 | 89 | 127 | 9 | 382 | 45 | 44 | 1,491 | 4 | 0 | 2 |
| 2026-01 | 1276820b | 623 | 204 | 91 | 126 | 11 | 449 | 46 | 45 | 1,505 | 5 | 0 | 2 |
| 2026-02 | 898ede41 | 609 | 218 | 92 | 127 | 12 | 821 | 47 | 46 | 1,506 | 6 | 0 | 2 |
| 2026-03 | 88c7055e | 639 | 243 | 102 | 126 | 19 | 889 | 51 | 53 | 1,766 | 8 | 0 | 0 |
| 2026-04 | e63ade78 | 682 | 245 | 121 | 132 | 17 | 946 | 53 | 55 | 1,835 | 9 | 0 | 5 |
| 2026-05 | 7ca7ee8f | 732 | 264 | 165 | 134 | 17 | 1,050 | 59 | 63 | 2,075 | 10 | 0 | 7 |
| 2026-06 | 2d917be7 | 786 | 268 | 219 | 141 | 17 | 1,181 | 57 | 59 | 2,062 | 12 | 0 | 11 |
| 2026-07 | 62b6a54a | 831 | 292 | 239 | 149 | 17 | 1,294 | 58 | 60 | 2,078 | 15 | 0 | 12 |
| 2026-08 | 413c952d | 875 | 296 | 243 | 158 | 18 | 1,358 | 60 | 62 | 2,138 | 16 | 0 | 12 |
| 2026-09 | 647b5b42 | 965 | 306 | 281 | 165 | 19 | 1,505 | 63 | 67 | 2,254 | 17 | 0 | 13 |
| 2026-10 | fe40c5a1 | 1,051 | 340 | 295 | 180 | 19 | 1,655 | 68 | 61 | 3,940 | 20 | 0 | 14 |
| 2026-11 | aa9c2d98 | 1,089 | 341 | 315 | 199 | 19 | 1,735 | 71 | 64 | 4,035 | 20 | 1 | 14 |

Note: the raw `tests/*.sql` count for 2026-11 is 21 because it includes `tests/generic/primary_key.sql` (added 2026-11-19, ec267008); it is shown as generic above.

What the curve shows:
1. Nothing but a template for the first three months (Feb to Apr 2025: 1 model yml, a handful of macros); the first real models (44, then 77 under a temporary `dbt_swf/` folder) appear in May and June 2025 and are re-laid out into the four layers in July 2025.
2. The Data Vault core (`edw_dv_bdv`) takes off twice: 135 to 486 models in October 2025 (the `odi-dev` import merged on 2025-10-09, 4ce61ff4, 432 added files; plus the INTEL-20532 standardisation) and again steadily from Mar 2026 (639) to Nov 2026 (1,089); `edw_ldm` jumps 47 to 191 in December 2025 (branch `feature/migratie-edw-ldm`, 0724739e, 143 models added on 2025-12-11); `im_dimensional` accelerates from Apr 2026 (121 to 315) and `pres_dataproduct` grows slowly but steadily (33 to 199).
3. Drops: `edw_dv_bdv` loses 14 models in Feb 2026 (623 to 609, inferred: clean-up while the grants/yml feature INTEL-22399, 3961c734, landed); seeds fall from 2 to 0 in Mar 2026 because the seeds were converted into Jinja models (8c54b3db, 2026-03-30) and come back from Apr 2026 (5 to 14) after the seed rework on 2026-05-05 (19af2f75); the `meta` folder (10 models) disappears in Aug 2026 and becomes `vdp_mgmt` (11) (inferred from the same-size swap); macro definitions fall 63 to 59 on 2026-06-22 when four dbt/adapter override macros were deleted after the dbt version upgrade (241b15c7), and 67 to 61 on 2026-10-27 when unused macros were removed in the documentation/reorganisation commit (ed2aea08).
4. Model yml files explode in Feb 2026 (449 to 821) because INTEL-22399 gave 327 models their own yml (grants moved out of the SQL `config()` into yml); they then follow the model count (about 0.9 yml per model sql).
5. Macro lines jump 2,254 to 3,940 in Oct 2026 without new logic: the commit "Standaarden beschrijven voor macros die we opleveren" (INTEL-25468, ed2aea08, 2026-10-27) added a documentation yml per macro folder (+2,173 lines, -580); the number of singular tests also triples (5 to 20) from Aug 2025 onwards but stays tiny next to 1,963 models.

## 2. Macro inventory with history

Folder history (when was the flat `macros/` reorganised):
- Flat folder from 2025-03-11 (95557ff6, first macros) until 2025-10-09: first sub-folders `core/` (dbt overrides), `utils/`, `hooks/`, `variables/` appear with the `odi-dev` import (first-parent 4ce61ff4, via 9ad40c9a): macro files 24 to 42 in one go.
- 2026-03-30 (8c54b3db, "Changes:"): four loose macros (`drop_old_relations`, `get_current_run_models`, `log_dbt_results`, `parse_dbt_results`) moved into `utils/`.
- 2026-10-27 (ed2aea08, merge fc7ca514, INTEL-25468): final reorganisation into 11 domain folders (business_rules, core, data_vault_hashing, data_vault_keys, data_vault_timeline, db_admin, incremental, model_helpers, pit, run_metadata, text_date_utils) with one yml documentation file per folder plus `macros/README.md`; 71 files, 41 renames of which most R60 to R100.

Totals: 102 macro-folder entities ever (after following renames), 71 live (13 of them yml/README), 31 deleted. Table columns: first added / commits touching (all branches) / last change / purpose. All first-add dates for 2025-10-09 mean "arrived with the odi-dev import". Purposes come from the documentation yml (Dutch) or the code.

### Live macros (today's folders)
| File | Added (hash) | Commits | Last change | Purpose |
|---|---|---|---|---|
| business_rules/comeet_release_nr_categories | 2026-03-10 9276ece2 | 3 | 2026-10-27 | map field `versie` to few categories (competence source, INTEL-22931) |
| business_rules/comeet_release_nr_beroep_categories | 2026-03-20 509b4cb3 | 2 | 2026-10-27 | same for the occupation source (INTEL-21553) |
| business_rules/competentstandaard_oms_categories | 2026-03-10 9276ece2 | 3 | 2026-10-27 | map `versie` to the competence standard |
| business_rules/competentstandaard_oms_beroep_categories | 2026-03-20 509b4cb3 | 2 | 2026-10-27 | same, occupation source |
| business_rules/generate_multi_active_satb_burger_alternatieve_naam | 2025-07-15 91aa30c0 | 7 | 2026-10-27 | multi-active satellite for a citizen's alternative occupation name (INTEL-18851) |
| business_rules/postcode_met_land_code | 2025-10-09 9ad40c9a | 2 | 2026-10-27 | postcode prefixed with country code (BE-9000) |
| core/generate_alias_name | 2025-04-01 df83559f | 6 | 2026-09-17 | override: object name = schema_name, prefix `__new_` on reload target |
| core/generate_database_name | 2025-03-11 95557ff6 | 3 | 2025-10-09 | override: custom database unless target `ci` |
| core/generate_schema_name | 2025-03-11 95557ff6 | 3 | 2025-10-09 | override: custom schema, `collibra` target uses database name |
| core/rename_relation | 2026-08-26 3b72159e | 1 | 2026-08-26 | adapter override (re-added; older version 2025-05-07 to 2026-03-30) |
| core/create_database | 2025-10-31 d99f0af3 | 1 | 2025-10-31 | adapter override: no-op, databases not created by dbt |
| core/create_view_as | 2026-08-26 3b72159e | 1 | 2026-08-26 | adapter override with sql_header and contract support |
| core/should_execute | 2025-10-09 9ad40c9a | 3 | 2026-10-27 | true only when dbt really runs SQL (build/run/..., not `--empty`) |
| data_vault_hashing/generate_md5_hash | 2025-07-01 f88e6b97 | 12 | 2026-10-27 | MD5 hash diff of columns with null handling, binary result |
| data_vault_hashing/build_lagged_hash_diff_cte | 2025-07-01 f88e6b97 | 7 | 2026-10-27 | keep only rows whose hash differs from the previous row |
| data_vault_hashing/generate_ikl_hash | 2025-06-23 79f3004f | 6 | 2026-10-27 | SHA1 of an IKL number with an environment salt |
| data_vault_keys/generate_hubc | 2025-06-25 7eabeaa8 | 3 | 2026-10-27 | hub with id/uuid and business-key split |
| data_vault_keys/split_bk_col | 2025-06-25 7eabeaa8 | 2 | 2026-10-27 | split `<bkcc>#<id>` business key |
| data_vault_keys/generate_x_cre_fields | 2025-03-11 3e95e0fe (flat) | 3 | 2026-10-27 | technical creation columns x_cre_dt, x_cre_batch_id |
| data_vault_timeline/generate_date_range | 2025-05-09 4c5d1e7a | 5 | 2026-10-27 | date range of all valid-from dates per key |
| data_vault_timeline/generate_date_range_join_condition | 2025-05-09 4c5d1e7a | 3 | 2026-10-27 | join a CTE on the date range |
| data_vault_timeline/generate_null_if_deleted | 2025-05-09 4c5d1e7a | 2 | 2026-10-27 | null a field when the delete flag is J |
| data_vault_timeline/generate_x_geldig_tot_dt | 2025-05-13 19434c3b | 7 | 2026-10-27 | valid-to = lead(valid-from), end 9999-12-31 |
| data_vault_timeline/generate_x_datum_dakey | 2025-03-11 3e95e0fe (flat) | 2 | 2026-10-27 | date to yyyymmdd key, -1 for null |
| data_vault_timeline/get_most_recent_cte | 2025-06-13 4246ca5c | 2 | 2026-10-27 | latest row per group |
| db_admin/cleanup_user_models | 2025-07-24 fad9beec | 4 | 2026-10-27 | drop a developer's own prefixed objects |
| db_admin/drop_nodes | 2025-08-26 87d78073 | 4 | 2026-10-27 | drop every object that is in the manifest |
| db_admin/perform_database_maintenance | 2026-03-03 072f7a4c | 6 | 2026-10-27 | find (dry run) or drop orphaned objects, fixed list of databases |
| db_admin/reload_helpers | 2026-09-17 762ddb4f | 2 | 2026-10-27 | blue/green reload: names, atomic RENAME swap, drop backup |
| incremental/get_incremental_window (+ incremental_* x4, 2026-05-15 a535d142) | 2026-05-15 | 1 to 2 | 2026-10-27 | start/end of the incremental window (default run time minus 7 days) and CTE builders |
| incremental/should_full_refresh | 2026-11-20 6c4e9db5 | 1 | 2026-11-20 | force full refresh for models listed in var `full_refresh_models` |
| model_helpers/import_dependencies, import_generate_cte | 2026-03-31 1fe862c5 | 2, 5 | 2026-10-27 | import ref/source CTEs at the top of a model |
| model_helpers/star | 2025-10-09 9ad40c9a | 2 | 2026-10-27 | select all columns except some |
| model_helpers/get_column_names, table_exists | 2026-10-27 ed2aea08 (moved in) | 1 | 2026-10-27 | helpers (older utils/ versions 2025-10 / 2026-07) |
| model_helpers/set_severity_error_except | 2026-11-19 ec267008 | 1 | 2026-11-19 | test severity error or warn by deploy target (INTEL-27197) |
| pit/pre_hook_fmc_pit_loading_history | 2025-12-17 36915fe6 | 7 | 2026-11-20 | clear load history of PITs before a run |
| pit/post_hook_pit | 2025-12-17 36915fe6 | 3 | 2026-10-27 | mark last load cycle as success in the PIT load history |
| pit/get_union_of_pit_run_models | 2025-12-17 36915fe6 | 3 | 2026-10-27 | union of all PIT objects |
| pit/check_full_refresh_pit_burger_db | 2025-10-09 9ad40c9a | 2 | 2026-10-27 | `--full-refresh` AND var `full_refresh_pit_burger_db` |
| pit/check_full_refresh_pit_burger_niet_beschikbaarheid_db | 2025-12-17 36915fe6 | 2 | 2026-10-27 | same for unavailability PIT |
| pit/check_full_refresh_fmc_pit_loading_history | 2026-01-16 7b3fb5fb | 2 | 2026-10-27 | same for the load history table |
| pit/check_full_refresh_pit_burger_indicatie_arbeidsbeperking_db | 2026-09-03 e670d0b5 | 2 | 2026-10-27 | same for disability-indicator PIT |
| pit/check_full_refresh_pit_medewerker_db | 2026-10-07 51d2e95f | 2 | 2026-10-27 | same for employee PIT (INTEL-25964) |
| pit/check_full_refresh_pit_arbeidsrelatie_financieel_product_db | 2026-11-20 dbd53f1a | 1 | 2026-11-20 | same for employment-relation product PIT (INTEL-27128) |
| run_metadata/log_dbt_results, parse_dbt_results | 2025-09-23 b777bf4f | 8, 8 | 2026-10-27 | write every run's results to `vdp_mgmt.dbt_run_results` (on-run-end) |
| run_metadata/get_current_run_models | 2025-12-17 36915fe6 | 3 | 2026-10-27 | list of models selected in this run |
| text_date_utils/add_time_to_date | 2025-07-17 c628dbeb | 2 | 2026-10-27 | date + time to timestamp |
| text_date_utils/get_first_day_of_current_month | 2025-07-28 ae5f437c | 2 | 2026-10-27 | first day of month |
| text_date_utils/format_sentence_case | 2026-06-04 f385ae86 | 2 | 2026-10-27 | BIT codes to readable sentences |
| text_date_utils/strip_html | 2026-06-30 74b2a71b | 2 | 2026-10-27 | HTML to plain text |
| text_date_utils/url_decode_common | 2026-09-25 257c9855 | 2 | 2026-10-27 | decode %20, + etc. |
| text_date_utils/generate_datetime | 2026-11-24 02faaf2f | 1 | 2026-11-24 | datetime helper (an earlier flat version lived 2026-10-28 to 2026-11-24) |
| 11 x `<folder>.yml` + README.md | 2026-10-27 ed2aea08 | 1 to 2 | 2026-11-24 | the INTEL-25468 documentation standard |

### Deleted macros (31 entities)
| Macro | Added | Commits | Deleted (hash) | What it was |
|---|---|---|---|---|
| test_varargs | 2025-05-09 4c5d1e7a | 2 | 2025-05-12 5157e9da | experiment |
| generate_id_bkcc | 2025-04-28 f3c8c2b6 | 5 | 2025-06-25 | key helper, replaced by hubc/split_bk_col |
| dummy_macro | 2025-07-28 c7104cbb | 2 | 2025-08-26 87d78073 | placeholder |
| hooks/post_hook_pit_burger_db, pre_hook_satc_burger_hb, update_fmc_pit_loading_history, update_fmc_pit_reloading_history; variables/determine_load_cycle_id_pit_burger, get_pit_burger_db_max_load_cycle_id (6) | 2025-10-09 9ad40c9a | 2 to 3 | 2025-12-17 36915fe6 | first PIT load-cycle mechanism, replaced by `pit/` hooks (INTEL-21252) |
| generate_x_geldig_van_dt | 2025-04-28 ffeac85b | 5 | 2026-03-10 8f1e6dfe | valid-from column (INTEL-22839) |
| core/create_table_as (first life) | 2025-07-25 9152c54a | 5 | 2026-03-30 8c54b3db | SingleStore table DDL override (reference/rowstore tables) |
| core/rename_relation (first life) | 2025-05-07 85d598e8 | 3 | 2026-03-30 8c54b3db | rename override (returned 2026-08-26) |
| core/create_table_as (second life) | 2026-06-05 82656b67 | 2 | 2026-06-22 241b15c7 | re-added for `--empty`, deleted by the dbt upgrade |
| core/get_delete_insert_merge_sql, grants, replace_view_definition, support_multiple_grantees_per_dcl_statement | 2025-04-01 to 2026-05-15 | 2 to 5 | 2026-06-22 241b15c7 | adapter workarounds made obsolete by the dbt upgrade |
| utils/sync_grants_with_singlestore | 2026-03-03 072f7a4c | 2 | 2026-10-27 | grant sync (replaced by yml grants) |
| utils/clean_orphaned_models, drop_old_relations, date_spine, get_column_names (old), get_current_run_models (old), table_exists (old) | 2025-10 to 2026-07 | 2 to 6 | 2026-10-27 ed2aea08 | merged into db_admin / run_metadata / model_helpers or dropped |
| variables/get_var, variables.yml, hooks/hooks.yml | 2025-10-09 | 2 to 5 | 2026-10-27 | superseded by the new yml per folder |
| generate_datetime (flat) | 2026-10-28 6d48f44f | 2 | 2026-11-24 02faaf2f | moved into text_date_utils |

## 3. The ten macros that changed most (all-branch commit count in brackets)

1. **generate_md5_hash [12]**: builds the change-detecting hash of every Data Vault satellite. 2025-07-01 f88e6b97 first as plain `md5(concat_ws('|', ...))`; 2025-07-18 2c8d5ba4 "unhex hashes" (binary instead of text), 2025-07-22 0abbf16a "undo binary hash for ikl hashing"; 2025-10-14 to 10-17 (bf8eb047, 5827ed85, b13d34c3, "Update after review") rewritten with null handling (`~` replacement), per-column type dictionary (timestamp, binary) and binary cast; 2026-05-19 9458305d "Casting aanpassen" (cast rewritten, +57/-45); 2026-10-09 b6e0dfbb INTEL-26675 replaced `cast(... as ...)` by SingleStore's `:>` operator. Why: every hash change would change every hash value; this is the macro where a "small" change touches the whole warehouse (inferred).
2. **log_dbt_results + parse_dbt_results [8 each]**: the run-results logging in the `on-run-end` hook. 2025-09-23 b777bf4f ("run_results") first version inserting into a dbt model; 2025-09-24 af1dd042 creates the table itself in `vdp_ci`; 2025-09-25 back to a depends_on ref; 2025-09-30 3b1ee1ba target `vdp_mgmt.dbt_run_results` with explicit column types; 2026-08-17 8b0e1392 adds `pre_hook`, `post_hook`, `compiled_code` columns; 2026-08-24 abadfb73 (INTEL-24214) quoting fix for the compiled code. The hook is guarded in `dbt_project.yml` by `LOG_RUN_RESULTS == TRUE` and target `run`.
3. **generate_alias_name [6]** (overrides a dbt internal): 2025-04-01 df83559f first; 2025-04-03 2baf1254/bddb60a6 "fixed" (object name = `<schema>_<name>` because SingleStore here uses databases, not schemas); 2025-09-18 816b7842 `collibra` target returns the plain node name; 2025-10-09 9ad40c9a moves into `core/`; 2026-09-17 762ddb4f ("Prep work for reload DAG") adds the `__new_` prefix on target `reload` so a rebuild is written next to the live table. Together with generate_database_name / generate_schema_name (2025-03-11 95557ff6; changed 2025-08-25 3066f3ff and 2025-09-18 816b7842).
4. **create_table_as / get_delete_insert_merge_sql / replace_view_definition / grants overrides** (adapter workarounds): 2025-04-01 9ac2f690 `replace_view_definition` "as temporary override until pull request into adapter is approved"; 2025-07-25 9152c54a `singlestore__create_table_as` to support reference and rowstore tables; 2026-02-13 3c533b96 and 2026-03-10 9e193994 grants support ("Fixed way dbt handles grants"); 2026-05-15 a535d142 `get_delete_insert_merge_sql` (delete_key for incremental); 2026-06-05 82656b67 "Fix dbt models breaking --empty flag"; 2026-06-22 241b15c7 (dbt version upgrade) deletes five of them (-283 lines).
5. **pre_hook_fmc_pit_loading_history + the `check_full_refresh_pit_*` family** [7 + 2 each, 6 files]: the manual "full refresh only the table I mean" gate: `--full-refresh` AND a per-table var in `dbt_project.yml`. One macro and one var per PIT: burger_db 2025-10-09 9ad40c9a, niet_beschikbaarheid 2025-12-17 36915fe6 (INTEL-21252), fmc_pit_loading_history 2026-01-16 7b3fb5fb (INTEL-21769), indicatie_arbeidsbeperking 2026-09-03 e670d0b5, medewerker 2026-10-07 51d2e95f (INTEL-25964), arbeidsrelatie_financieel_product 2026-11-20 dbd53f1a (INTEL-27128). The same day a generic replacement appears: `should_full_refresh` + var `full_refresh_models` (6c4e9db5, "controle individuele full refreshes via vars"), a list on the command line instead of one macro per table.
6. **should_execute [3]**: 2025-10-09 first; 2026-06-05 82656b67 adds `and not flags.EMPTY` so hooks and PIT bookkeeping do not run on `--empty` (the CI verification run added with `--empty` on 2025-12-11 5cf3460f).
7. **perform_database_maintenance (+ sync_grants_with_singlestore, cleanup_user_models, drop_nodes, drop_old_relations, clean_orphaned_models) [6, 4, 4, 3, 6]**: the "tidy the warehouse" family. cleanup_user_models 2025-07-24 fad9beec (drop developer-prefixed objects); drop_nodes 2025-08-26 87d78073; clean_orphaned_models 2025-12-08 07134533, dry-run mode 2025-12-19 ae14a867; perform_database_maintenance 2026-03-03 072f7a4c, JSON output 2026-03-04 84de52a3, ignore dbt temporary/backup objects 2026-05-22 71f36459, clean dbt artifacts 2026-06-19 dc11af86; all folded into `db_admin/` 2026-10-27.
8. **build_lagged_hash_diff_cte [7]**: 2025-07-01 f88e6b97; 2025-10-15 a95f60fe "Extend macro"; 2025-10-17 b13d34c3 "Update after review other data engineers" and a82def4f multi-column partition/order; 2025-10-23 6c25cca0 datetime cast fix. Fed by the review comments of colleagues (the commit says so).
9. **generate_x_geldig_tot_dt / generate_x_geldig_van_dt [7 / 5]** (valid-from/valid-to timeline columns): 2025-04-28 ffeac85b and 2025-05-13 19434c3b first; 2025-06-23 79f3004f review; 2025-10-27 4477a06e "replace datetime lit" (end date `to_date('31/12/9999', ...)` becomes `'9999-12-31'`, together with INTEL-20578 on 322 files); geldig_van deleted 2026-03-10 8f1e6dfe (INTEL-22839).
10. **Incremental framework** (6 new macros 2026-05-15 a535d142 "Squashed commit": `get_incremental_window`, `incremental_*`, `get_delete_insert_merge_sql`; same squash: "Added delete_key config option for incremental models instead of misused unique_key", "Removed backfill macro to avoid misuse", first `satc_burger_hb_incr` "to run in parallel with the original"); 2026-11-20 6c4e9db5 `should_full_refresh` override.

Also noted: `reload_helpers` 2026-09-17 762ddb4f (blue/green swap with `__new_` prefix, `get_reload_relation_names`, `reload_atomic_swap` = `RENAME TABLE orig TO backup, new TO orig`, `reload_drop_backup`) alongside the new `reload` target and `selectors.yml`.

## 4. Project files over time

### dbt_project.yml
Lines at quarter end (last first-parent commit): 2025-Q1 17 (f2adb1f9), Q2 66 (dde222a2), Q3 62 (2cc985ba), Q4 79 (dd88a910), 2026-Q1 84 (88c7055e), Q2 98 (2d917be7), Q3 121 (647b5b42), 2026-11-26 152. 50 first-parent commits (66 on all branches) touched it.

- Project name: `dbt_singlestore_project` (initial, 801f119b 2025-02-26) -> `dbt_template` (95557ff6 2025-03-11) -> `dbt_swf` (7e763eeb). 2026-05-15 a535d142 "Renamed project from dbt-swf to dbt-vdp"; the same day 23f1c061 "Fixed project name back to dbt_swf" (a rename the pipeline did not survive; the repo itself is now called dbt-vdp). 2026-01-02 3d866be8 "Make project importable as package" moves all vars under the project-name key.
- Vars (date, hash, ticket): kow_switch_date 2025-04-24 9b6bc8ee (added, removed 2025-06-26 14d11e0e, back 2025-06-27 0102fd1a INTEL-18454, removed 2025-06-29 52a3201f INTEL-18883); dbt_date (time zone Europe/Brussels) 2025-04-28 0812f76d; id/uuid/bkcc_column_length 2025-05-06 b0ad741e ("hubc refactor"; removed by a branch 2025-05-12 and restored 2025-05-13 3ca6c8aa "sync main"); full_refresh_pit_burger_db + pit_burger_db_load_cycle_id 2025-10-09 9ad40c9a; full_refresh_pit_burger_niet_beschikbaarheid_db 2025-12-17 36915fe6 INTEL-21252 (load_cycle_id var removed); full_refresh_fmc_pit_loading_history + max_nr_days_batch_pit 2026-01-16 7b3fb5fb INTEL-21769; local_run_started_at 2026-02-11 43d0d353 INTEL-21467; full_refresh_pit_medewerker_db 2026-10-07 51d2e95f INTEL-25964; full_refresh_pit_arbeidsrelatie_financieel_product_db 2026-11-20 dbd53f1a INTEL-27128; the indicatie_arbeidsbeperking full-refresh var is in the file today (macro 2026-09-03 e670d0b5).
- Cut-off date vars (per environment LDV/ABT/CBT/PRD, the switch-over moment of a changed feed): start_organisatie_label_v3_2026_dt 2026-05-04 3d843381 INTEL-23700; bit_cutoff_date_2026_dt 2026-05-20 4ec489c0 INTEL-23866; start_correctie_nis_code_2026_dt 2026-07-02 34df7ee3; start_correctie_landmon_origine_2026_dt 2026-07-20 384426b9; burger_switch_sas_vdp_dt 2026-07-28 7f2df391 INTEL-20932; burger_afstand_switch_sas_vdp_dt 2026-08-10 57a7509f INTEL-24159; start_brgdvl_werkloosheidsverzekering_crud_dt 2026-08-28 b56f8ebf; start_brgdvl_indicatie_arbeidsbeperking_crud_dt 2026-10-22 11e8b069 INTEL-26840; start_vacature_c2_dt 2026-11-23 27e8d978. 14 vars today.
- Hooks: `on-run-end: log_dbt_results` added 2025-09-23 b777bf4f, adjusted 2025-09-25, 2025-09-30, 2026-04-28 3a78f3b8 ("Fixed warnings in project") and 2026-05-15 a535d142 (guarded by `LOG_RUN_RESULTS` and target `run`). No pre-hook/post-hook at project level; model-level hooks live in the PIT macros. Not found: removal of any hook.
- Flags: `use_colors`, `send_anonymous_usage_stats` 2025-04-10 bd7d15f3 ("Relocate config flags"); `require_generic_test_arguments_property: true` 2025-12-22 431bfd59. No `static_analysis` flag anywhere in history (`g log -S`).
- Quoting: database/schema/identifier all true since 2025-04-28 0812f76d, never changed.
- Default test severity: `data_tests: +severity: warn` since 2025-10-09 9ad40c9a; not changed afterwards, but individual tests flip by environment through `set_severity_error_except` (2026-11-19 ec267008, INTEL-27197).
- Other: query-comment (`run by {{ target.user }}`) 2025-04-28; `seeds:` block added 2025-11-20, removed 2026-06-22, re-added 2026-07-02 and removed again, re-added 2026-10-27 (a3d43e72) because seed column types in yml were dropped for Fusion; `log-path`/`target-path` removed 2025-12-22.
- 2026-08-05 2027a1dc: the `+group` entries per folder were replaced by `+tags` (the folder restructure, section 6).

### packages.yml
- Only one registry package, ever: `dbt-labs/dbt_utils` (>=1.3.0 from 801f119b 2025-02-26; >=1.3.3 on 2025-12-18 52cd65a0; reformatted 2026-05-12 cf48a9dc "dbt on k8s"). Not found: dbt_date, elementary, codegen or any other package.
- One git package, the in-house base project `dbt-singlestore-base` (the base macros): present from 2025-02-26, URL switched to ssh 2025-03-25 18c76b2d, pinned to a revision 2025-05-06 1d90b975, removed 2025-10-24 493663ee ("Updated dbt base image": the content now ships inside the Docker base image).

### profiles.yml (targets)
| Date | hash | Targets |
|---|---|---|
| 2025-02-26 | 801f119b | `ss_db` |
| 2025-03-11 | 95557ff6 | `default` (single target) |
| 2025-05-10 | 3b066a19 | `default`, `local` (local SingleStore container) |
| 2025-08-21 | 0905fd23 | `ci`, `run`, `default` (`local` gone) |
| 2025-09-18 | 816b7842 | + `collibra` (schema compilation for the data catalog) |
| 2025-10-09 | 9ad40c9a | `collibra` disappears again (a merge overwrote it) |
| 2025-10-28 | 64f145ce | `collibra` back ("Merge branch 'collibra'") |
| 2026-09-17 | 762ddb4f | + `reload` (blue/green rebuild target) |
| 2026-10-27 | a3d43e72 | Fusion prep: `ssl_mode: required`, `allow_cleartext_plugin: true` |
Also: `threads=1` 2025-09-05 aee272b4; performance tuning 2026-07-10 18c2ee18 (INTEL-25502, "DBT C2 PERFORMANCE VERBETERINGEN").

### selectors.yml
Introduced 2026-09-17 762ddb4f (same commit as the `reload` target) with three selectors: `daily_load` (all models and seeds, tests indirect selection `empty`, excludes tag `on-demand`), `critical_tests`, `non_critical_tests`. 2026-10-19 432b08f9 "Fixed daily load selector": `daily_load` becomes `default: true`, indirect selection `buildable`, and excludes the `non_critical_tests` selector. Usage in the header comment: `dbt build --selector daily_load --warn-error`.

## 5. dbt engine changes
- Base-image pin history (Dockerfile / CI / devcontainer): "Using VDAB dbt base image" 2025-05-06 b98d7d22; pinned versions 2025-07-14 db71c6cd and 2025-08-18 95e6ffb9; new base image 2025-10-24 (493663ee, 00c8fff6, 11c6e206; the git package leaves packages.yml); renewed 2025-11-20 0facf173; "Update base image to 1.9.1" 2025-11-28 bfca269f; tag 1.9.2 in `dev.Dockerfile` on 2026-02-18 343fdd6c (first Dockerfile in the repo); date-stamped tags 20260227.122 to 20260310.128 (trial and error: `dev.Dockerfile` has 48 commits, 18 of them between 2026-03-01 and 2026-03-10, mostly "Edit dev.Dockerfile"); **1.10.20 on 2026-03-30** (8c54b3db "dbt container geupdate naar dbt 1.10.20"; message warns developers must update because `source freshness` needs the new image); **1.11.11 on 2026-06-22/23** (241b15c7 "Feature/dbt version upgrade", 13 files, -283 lines, mostly removal of adapter override macros; 120db3f7 fixes the version in the Jenkins Docker files). docs image: nginx base 3.2.10 -> 3.4.18 (2026-06-12 c4e495ab) -> 3.4.33 (2026-09-18 d48d5d36).
- Test-argument syntax migration: 2025-12-22 431bfd59 "Project formatting + yml autofix" (merge 70f83680) touched 887 files (880 modified, 554 of them in edw_dv_bdv) and added about 258 `arguments:` blocks in model yml (dbt 1.10 syntax) together with the flag `require_generic_test_arguments_property: true`. Since the image was still tagged 1.9.x then, this was preparation for 1.10 (inferred).
- Fusion: branch `fusion` merged 2026-10-27 (a3d43e72, merge 8c74deae, 26 files, +25/-87): removed per-seed column-type yml (11 seed yml files), added `seeds:` config and the two profile settings above. No `static_analysis` anywhere. The same day macro reorganisation INTEL-25468 landed. An Airflow/k8s test file `tests/integration/test_dbt_vdp_k8s.py` and "dbt on k8s" (2026-05-12 cf48a9dc) show the runtime moved to Kubernetes in May 2026.
- CI tool moves: Jenkinsfile 2025-03-25 (57a6f4f7), GitLab CI file 2025-05-02 (81ab1595); both still referenced in 2026 (Jenkins docker files), i.e. a coexistence (inferred).

## 6. Big-bang commits (first-parent on `dev`, ranked by files changed)
| # | Date | Hash | Subject | Files | +/- | What it was |
|---|---|---|---|---|---|---|
| 1 | 2026-08-05 | 2027a1dc | Merge feature/INTEL-25478 | 2,186 | +37/-20 | Pure restructuring: 2,183 renames (R100 for 2,171). `edw_dv_bdv/conform,werk` into `satc, satb, refc, lksb, hubc, brg`; `im_dimensional/presentatie,persistentie` into `da, ftr`; `pres_dataproduct/jobmatch, kans_op_werk` into `dp_ai`; project yml: `+group` replaced by `+tags` |
| 2 | 2025-12-22 | 70f83680 | Merge feature/project-formatting | 887 | +18,463/-16,546 | Formatting plus yml autofix: sqlfmt/yml layout, argument syntax (section 5) |
| 3 | 2026-09-04 | d93a443b | Merge hotfix/project_force_format | 843 | +8,545/-3,226 | Forced SQL format on all models (one-line `select ... from` blown up into several lines) |
| 4 | 2026-02-13 | 3961c734 | Merge Feature/INTEL-22399 | 836 | +2,851/-41,173 | Grants moved from SQL config into model yml (327 yml created, 500 modified, `python/` helper scripts deleted); yml lists collapsed to one line |
| 5 | 2026-03-26 | cb358b4e | Merge 'dev' from dbt-swf into dev | 657 | +10,855/-4,343 | Sync of 175 commits from the other team's repo (138 files added, 488 modified) |
| 6 | 2025-10-09 | 4ce61ff4 | Merge odi-dev into main | 511 | +21,858/-153 | Import of a whole parallel line of work: 432 files added (models, 25 macros) and the macro sub-folders |
| 7 | 2025-11-25 | 18dee2ff | Revert INTEL-20532 standaardisatie | 333 | +7,619/-38,204 | Revert of #8 the day after |
| 8 | 2025-11-24 | 5fd40569 | Merge feature/INTEL-20532 | 333 | +38,204/-7,619 | "Standardisation": SQL rewritten into CTE style, a 28,000-line source yml; reverted next day |
| 9 | 2025-12-01 | bfd031ad | Merge feature/INTEL-20532 | 327 | +9,826/-7,596 | Second attempt at the same standardisation |
| 10 | 2025-12-02 | 3a52f715 | Revert (of #9) | 327 | +7,596/-9,826 | Reverted again the day after; fixed by bugfix merge 5b9e5003 on 2025-12-03 |
| 11 | 2025-10-31 | 597e2486 | Merge feature/INTEL-20578 | 322 | +680/-679 | One-line change in 322 files: Oracle-style `to_date('31/12/9999','DD/MM/YYYY')` replaced by `'9999-12-31'` |
| 12 | 2026-04-28 | e63ade78 | Merge dev-swf into dev | 251 | +41,519/-10,979 | Sync of 147 commits (2026-03-09 to 04-28) from the other repo, 10 seeds |
| 13 | 2026-05-08 | 4ab47a86 | Merge dev-swf into dev | 218 | +13,244/-33,065 | Same sync line, 105 added, 14 deleted (seeds reworked) |
| 14 | 2025-10-28 | 84ef6f87 | Merge feature/INTEL-20532-squash | 153 | +7,819/-6,209 | First attempt of the standardisation (squashed) |
| 15 | 2025-12-11 | 0724739e | Merge feature/migratie-edw-ldm | 144 | +2,710/-0 | 143 `edw_ldm` models added at once (migration) |
Next in line: 2025-11-07 de8fa243 (release/2025Q4_C3 merge, 111 files).

## Stories worth telling

1. **The standardisation that was merged, undone, merged and undone again (INTEL-20532, 24 Nov to 3 Dec 2025).** A project-wide rewrite of 333 models was merged on 2025-11-24 (5fd40569), reverted the next day (18dee2ff), tried again on 2025-12-01 (bfd031ad), reverted on 2025-12-02 (3a52f715) and only landed after a bugfix on 2025-12-03 (5b9e5003). A first squashed version had already gone in on 2025-10-28 (84ef6f87). One feature cost four merges and two reversals in ten days. Workshop angle: rehearse before merge, and be able to roll a change back with one step.
2. **2,186 files moved in one commit, only 57 lines of real change (2026-08-05, 2027a1dc, INTEL-25478).** All models were reorganised from folders named after process (conform, werk, presentatie, persistentie) to folders named after Data Vault type (satc, satb, refc, hubc, ...). Because git saw 2,171 pure renames, history and review stayed readable. Workshop angle: house rules for folder structure, and rename without changing content in the same commit.
3. **Two mass "make it pretty" days (2025-12-22, 887 files; 2026-09-04, 843 files).** The first autoformat also moved tests to a new syntax; the second, "hotfix/project_force_format", was needed because formatting had drifted again. Nearly 1,700 file touches with zero business change. Workshop angle: house rules / automatic formatter checked in every merge request, instead of periodic clean-ups.
4. **A date written the Oracle way broke nothing but touched 322 files (2025-10-31, 597e2486).** The last valid-to date `31/12/9999` was written as `to_date('31/12/9999','DD/MM/YYYY')` in hundreds of models inherited from an older platform; one reviewed change replaced it by `'9999-12-31'`. Workshop angle: automatic checks that flag legacy patterns, plus one shared macro for constants that every model needs.
5. **Rebuild one table, not everything: from six tailor-made switches to one list (2025-10 to 2026-11).** For each big history table (PIT) the team wrote a small macro and a switch in `dbt_project.yml` that only lets a full refresh through for that table (first 2025-10-09, sixth 2026-11-20). On 2026-11-20 (6c4e9db5) a general `full_refresh_models` list replaced the need for another one. Also the blue/green reload target (`reload`, `__new_` prefix, atomic rename, 2026-09-17 762ddb4f). Workshop angle: rebuild without downtime, redo only what failed.
6. **Developers dropped their own objects into the shared database, so a tidy-up toolkit grew (2025-07 to 2026-06).** `cleanup_user_models` (2025-07-24), `drop_nodes`, orphan detection with dry-run (2025-12-19) and `perform_database_maintenance` (2026-03-03) that finds and deletes objects nobody's model owns. Workshop angle: own workbench per developer, with automatic cleanup and dry run before delete.
7. **A feature that vanished in a merge and came back (2025-10-09 / 2025-10-28).** The `collibra` target (documentation/catalog compilation) was added to `profiles.yml` on 2025-09-18 (816b7842), silently lost when the big `odi-dev` branch was merged on 2025-10-09, and restored on 2025-10-28 (64f145ce). Workshop angle: one team not ready or out of sync at release time; automatic checks that compare what is on the main line with what should be there.
8. **Access rights as code: 327 models got a yml with their grants in one go (2026-02-13, 3961c734, INTEL-22399).** Rights for roles (`select` for monitoring/planning roles) were taken out of the SQL and set in each model's yml; the first grant-handling macros followed on 2026-03-03 and a fix on 2026-03-10 (9e193994); in June 2026 the dbt upgrade removed the workaround macros (241b15c7). Workshop angle: access rights, defined next to the model and applied automatically.
9. **Every run now leaves a receipt (2025-09-23 to 2026-08-24).** The `on-run-end` hook writes status, timing, rows and (since 2026-08-17, 8b0e1392) the compiled SQL of every model into a management table; it took five commits in one week to settle how the table is created and where it lives (first in a CI database, then in `vdp_mgmt`, 2025-09-30 3b1ee1ba). Workshop angle: every run on record, failure mail built on top of it.
10. **Changing a feed on a given day: one date per environment (2026-05 to 2026-11).** Nine variables hold the "from this moment on, read the new source" timestamp, one per environment (LDV, ABT, CBT, PRD), e.g. new organisation label topic 2026-03-25 in test and 2026-04-08 in production (INTEL-23700, 3d843381). Each var is added in the same change as the model that needs it. Workshop angle: switch-over date for a changing feed, tested in every environment before production.
11. **Same-day rename and rollback (2026-05-15).** The squash a535d142 renamed the project dbt-swf to dbt-vdp; the same day 23f1c061 "Fixed project name back to dbt_swf". Its commit text even says "changes to dbt_project.yml build everything apparently" (a change to the project file marks all models as changed). Workshop angle: numbered versions and rollback; keep project-level files stable.

Note for the workshop author: none of the repository text contained instructions addressed to an assistant; nothing was executed from the repository.
