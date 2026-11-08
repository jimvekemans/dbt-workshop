"""Central configuration and environment constants for Airflow dbt DAGs."""

import os

HOST = os.environ.get("IFW_PILLAR", "rel1ldv")
ENV = HOST.replace("rel1", "")
ENV_UPPER = ENV.upper()

NAMESPACE = f"{ENV}-vdp-airflow"
KUBE_CONFIG = "/opt/apache_airflow/.kube/config"

HOST_DNS_SUFFIX_MAP = {
    "rel1ldv": "ops.vdab.be",
    "rel1abt": "ops.vdab.be",
    "rel1cbt": "vdab.be",
    "rel1prd": "vdab.be",
}
HOST_DNS_SUFFIX = HOST_DNS_SUFFIX_MAP.get(HOST, "ops.vdab.be")

# Shared PVC storage paths
MOUNT_DIR = "/mnt/dbt"
TARGET_DIR = os.path.join(MOUNT_DIR, "target")
ARTIFACTS_DIR = os.path.join(MOUNT_DIR, "artifacts")

DAILY_ARTIFACTS_DIR = os.path.join(ARTIFACTS_DIR, "daily_load")
RELEASE_ARTIFACTS_BASE_DIR = os.path.join(ARTIFACTS_DIR, "release")
RELOAD_ARTIFACTS_BASE_DIR = os.path.join(ARTIFACTS_DIR, "reload")

SAFE_RUN_ID_TEMPLATE = "{{ run_id | replace(':', '_') | replace('+', '_') }}"

# Image & Secret naming
SECRET_NAME = f"s-{ENV}-vdp-dbt"
DBT_IMAGE = f"artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:{ENV}"
PVC_CLAIM_NAME = "pvc-vdp-airflow"
DBT_PV_STORAGE_NAME = "dbt-pv-storage"
