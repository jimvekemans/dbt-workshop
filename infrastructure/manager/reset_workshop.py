#!/usr/bin/env python3
"""
Reset the workshop to its baseline: drop and re-create the environment databases (dev, val, prd) and vdp_ci, reset the
GitLab repository to the baseline (dev + main + first tag, which starts the first pipeline again) and clear the Airflow
Variables used for runtime overrides. Runs in the manager container: `make reset`.
"""

import subprocess
import sys

try:
    import pymysql  # noqa: F401
except ImportError:
    sys.exit("Missing deps (PyMySQL, requests): run it in the manager container - make reset")

from bootstrap_stack import (
    ENV_DATABASES,
    ENVS,
    ROOT_PAT,
    apply_sql,
    ensure_ci_job_token_signing_key,
    latest_pipeline_id,
    push_baseline,
    seed_gitlab,
    wait_for_pipeline,
    wait_for_singlestore,
)

# Airflow Variables used as runtime levers between deploy windows (airflow/utils/dbt_runtime_overrides.py).
# dbt_image_tag is not cleared: the deploy of the first tag sets it again, and Airflow keeps running meanwhile.
LEVER_VARIABLES = ("disabled_dbt_models", "dbt_daily_load_dbtignore", "dbt_env_vars")


def reset_singlestore():
    print("[1/3] Dropping and re-creating the environment databases and vdp_ci...")
    conn = wait_for_singlestore()
    with conn.cursor() as cur:
        for db in [f"{env}_{name}" for env in ENVS for name in ENV_DATABASES] + ["vdp_ci"]:
            cur.execute(f"DROP DATABASE IF EXISTS `{db}`")
        apply_sql(cur)
    conn.close()


def clear_lever_variables():
    for env in ENVS:
        for key in LEVER_VARIABLES:
            subprocess.run(["docker", "exec", f"workshop-airflow-{env}", "airflow", "variables", "delete", key],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
    print(f"  -> Airflow Variables cleared in the three Airflows: {', '.join(LEVER_VARIABLES)}")


def main():
    print("=== Resetting the dbt workshop to the baseline ===")
    reset_singlestore()
    print("[2/3] Resetting the GitLab repository...")
    ensure_ci_job_token_signing_key()
    _, pid = seed_gitlab(ROOT_PAT, register_runner=False)
    previous = latest_pipeline_id(ROOT_PAT, pid)
    push_baseline(ROOT_PAT, pid)
    print("[3/3] Clearing the Airflow runtime overrides...")
    clear_lever_variables()
    wait_for_pipeline(ROOT_PAT, pid, newer_than=previous)
    print("=== Reset complete ===")


if __name__ == "__main__":
    main()
