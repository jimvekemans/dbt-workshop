"""Central configuration for the Airflow dbt DAGs. Everything environment-specific derives from DBT_DEPLOY_TARGET."""

import os

ENVIRONMENTS = ("dev", "val", "prd")
ENV = os.environ.get("DBT_DEPLOY_TARGET", "dev").strip().lower() or "dev"
if ENV not in ENVIRONMENTS:
    raise ValueError(f"DBT_DEPLOY_TARGET must be one of {ENVIRONMENTS}, got {ENV!r}")

# Database dbt connects to (profiles.yml, targets default/run/reload)
SINGLESTORE_DB = f"{ENV}_edw_dv_rdv"

# Shared state volume, mounted in this container and (subpath mnt_dbt) in every dbt container
MOUNT_DIR = "/mnt/dbt"
TARGET_DIR = os.path.join(MOUNT_DIR, "target")
ARTIFACTS_DIR = os.path.join(MOUNT_DIR, "artifacts")

DAILY_ARTIFACTS_DIR = os.path.join(ARTIFACTS_DIR, "daily_load")
RELEASE_ARTIFACTS_BASE_DIR = os.path.join(ARTIFACTS_DIR, "release")
RELOAD_ARTIFACTS_BASE_DIR = os.path.join(ARTIFACTS_DIR, "reload")

SAFE_RUN_ID_TEMPLATE = "{{ run_id | replace(':', '_') | replace('+', '_') }}"

# The dbt project runs from the CI-built image dbt-vdp:<tag>; the tag is the Airflow Variable set by every deploy
DBT_IMAGE_REPOSITORY = "dbt-vdp"
DBT_IMAGE_TAG_VARIABLE = "dbt_image_tag"
IMAGE_WORKDIR = "/usr/app/dbt/workspace"
STATE_VOLUME_SUBPATH = "mnt_dbt"
