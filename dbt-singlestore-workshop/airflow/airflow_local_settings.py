import os
import logging
import warnings
from urllib3.exceptions import InsecureRequestWarning

from airflow.providers.cncf.kubernetes.utils.xcom_sidecar import PodDefaults
from kubernetes.client import models as k8s

warnings.filterwarnings("ignore", category=InsecureRequestWarning)
logging.getLogger("urllib3.connectionpool").setLevel(logging.ERROR)


class SilenceLogInterruptionFilter(logging.Filter):
    def filter(self, record: logging.LogRecord) -> bool:
        if (
            "log read interrupted but container base still running"
            in record.getMessage()
        ):
            return False
        return True


class SilenceInsecureRequestsFilter(logging.Filter):
    def filter(self, record: logging.LogRecord) -> bool:
        if "InsecureRequestWarning" in record.getMessage():
            return False
        return True


pod_manager_logger = logging.getLogger(
    "airflow.providers.cncf.kubernetes.utils.pod_manager"
)
pod_manager_logger.addFilter(SilenceLogInterruptionFilter())
pod_manager_logger.addFilter(SilenceInsecureRequestsFilter())
pod_manager_logger.setLevel(logging.INFO)

hosts = {
    "rel1ldv": "LDV",
    "rel1abt": "ABT",
    "rel1cbt": "CBT",
    "rel1prd": "PRD",
}

pillar = os.getenv("IFW_PILLAR", "rel1ldv")
env = hosts.get(pillar, "LDV").lower()


PodDefaults.SIDECAR_CONTAINER = k8s.V1Container(
    name=PodDefaults.SIDECAR_CONTAINER_NAME,
    command=["sh", "-c", PodDefaults.XCOM_CMD],
    image=f"artifacts.vdab.be/cops-docker-vdab/ondersteuning/dbt-vdp/dbt-vdp:{env}",
    volume_mounts=[PodDefaults.VOLUME_MOUNT],
    resources=k8s.V1ResourceRequirements(
        requests={"cpu": "10m", "memory": "32Mi"},
        limits={"cpu": "0.25", "memory": "32Mi"},
    ),
)
