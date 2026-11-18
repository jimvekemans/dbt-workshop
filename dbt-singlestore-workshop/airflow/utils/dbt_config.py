"""Central configuration for the Airflow dbt DAGs. Everything environment-specific derives from DBT_DEPLOY_TARGET."""

import os

# Where the dbt tasks run: "docker" (the workshop stack: sibling containers of the Airflow container) or "openshift"
# (pods started with the Kubernetes provider). See utils/dbt_pod_utils.py.
RUNTIMES = ("docker", "openshift")
RUNTIME = os.environ.get("DBT_RUNTIME", "docker").strip().lower() or "docker"
if RUNTIME not in RUNTIMES:
    raise ValueError(f"DBT_RUNTIME must be one of {RUNTIMES}, got {RUNTIME!r}")

ENVIRONMENTS = ("dev", "val", "prd")
# On OpenShift the environment may also come from the platform's pillar name (rel1ldv -> ldv)
_PILLAR_ENV = os.environ.get("IFW_PILLAR", "").strip().lower().replace("rel1", "")
ENV = (os.environ.get("DBT_DEPLOY_TARGET", "").strip().lower() or _PILLAR_ENV or "dev")
if RUNTIME == "docker" and ENV not in ENVIRONMENTS:
    raise ValueError(f"DBT_DEPLOY_TARGET must be one of {ENVIRONMENTS}, got {ENV!r}")

# What the dbt containers get as DBT_DEPLOY_TARGET
DEPLOY_TARGET = ENV

# Database dbt connects to (profiles.yml, targets default/run/reload), passed as SINGLESTORE_DB
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


def _setting(name: str, default: str) -> str:
    return os.environ.get(name, "").strip() or default


# OpenShift runtime (DBT_RUNTIME=openshift). The defaults are those of the VDAB cluster; every one can be overridden
# with the environment variable of the same name.
OPENSHIFT_NAMESPACE = _setting("DBT_OPENSHIFT_NAMESPACE", f"{ENV}-vdp-airflow")
# An empty value means: no kube config file, use the in-cluster service account / the Kubernetes connection
OPENSHIFT_KUBE_CONFIG = os.environ.get("DBT_OPENSHIFT_KUBE_CONFIG", "/opt/apache_airflow/.kube/config").strip() or None
OPENSHIFT_SECRET_NAME = _setting("DBT_OPENSHIFT_SECRET_NAME", f"s-{ENV}-vdp-dbt")
# Keys of the secret a pod may start without (the others are required)
OPENSHIFT_OPTIONAL_SECRET_KEYS = ("IKL_HASH_SALT",)
OPENSHIFT_PVC_CLAIM_NAME = _setting("DBT_OPENSHIFT_PVC_CLAIM_NAME", "pvc-vdp-airflow")
OPENSHIFT_PVC_SUBPATH = _setting("DBT_OPENSHIFT_PVC_SUBPATH", "dbt")
OPENSHIFT_IMAGE_REPOSITORY = _setting(
    "DBT_OPENSHIFT_IMAGE_REPOSITORY", "artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp"
)
# Rendered by Airflow when the task runs: the deployed tag (Variable dbt_image_tag) or, when no tag was deployed, the
# moving tag named after the environment.
OPENSHIFT_IMAGE = _setting(
    "DBT_OPENSHIFT_IMAGE",
    f"{OPENSHIFT_IMAGE_REPOSITORY}:{{{{ var.value.get('{DBT_IMAGE_TAG_VARIABLE}', '{ENV}') }}}}",
)
# Image of the pods that only work on the artifacts (python_task): it needs python3, nothing else
OPENSHIFT_STATE_IMAGE = _setting("DBT_OPENSHIFT_STATE_IMAGE", OPENSHIFT_IMAGE)
