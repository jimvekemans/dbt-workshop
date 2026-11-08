"""Dataset outlet and Cronacle integration utilities for Airflow dbt workflows."""

from base64 import b64encode
import logging
from typing import Any
import requests


from utils.compat import AirflowException, Dataset, DatasetAlias, get_variable, task
from utils.dbt_config import HOST, ENV, HOST_DNS_SUFFIX

logger = logging.getLogger(__name__)

DBT_MODELS_ALIAS = DatasetAlias("dbt_dataset_output_alias")


def get_outlet_path(*args: str) -> str:
    """Get the S2 dataset URI path for a given database and object (table or view)."""
    if len(args) == 2:
        db, object_name = args
    elif len(args) == 1 and args[0].count("/") == 1:
        db, object_name = args[0].split("/")
    else:
        raise ValueError(
            "get_outlet_path expects either (db, object) or a single 'db/object' string"
        )

    return f"s2://singlestore-aggregator00-{HOST}.{HOST_DNS_SUFFIX}:3306/{db}/{object_name}"


@task(
    task_id="generate_dataset_outlets",
    outlets=[DBT_MODELS_ALIAS],
    trigger_rule="all_done",
)
def generate_dataset_outlets_task(
    results: dict[str, Any], outlet_events: Any = None, **context: Any
) -> None:
    """Generate Airflow dataset outlet events dynamically based on run results."""
    relation_results = results.get("results", []) if isinstance(results, dict) else []
    for result in relation_results:
        db = result["db"]
        _object = result["object"]
        dataset = Dataset(get_outlet_path(db, _object))
        logger.info(f"Generating dataset event for {db}/{_object}")
        if outlet_events is not None and DBT_MODELS_ALIAS in outlet_events:
            outlet_events[DBT_MODELS_ALIAS].add(
                dataset,
                extra={
                    "created_by_dag_id": context["dag"].dag_id,
                    "created_by_run_id": context["run_id"],
                },
            )


@task(
    task_id="trigger_cronacle_vdpreadysignal",
    trigger_rule="none_failed_min_one_success",
)
def trigger_cronacle_vdpreadysignal_task() -> dict[str, Any]:
    """Send SOAP request to Cronacle signaling that VDP data processing is complete."""
    url = get_variable("cronacle_soap_url", None)
    if not url:
        logger.info(
            "Cronacle Variable 'cronacle_soap_url' is not configured; skipping external SOAP signal."
        )
        return {"status": 200, "text": "skipped (no cronacle_soap_url configured)"}

    soap_user = get_variable("secret_cronacle_soap_user", None)
    soap_passwd = get_variable("secret_cronacle_soap_passwd", None)
    if not soap_user or not soap_passwd:
        raise AirflowException(
            "Cronacle credentials not configured: Airflow Variables "
            "'secret_cronacle_soap_user' and/or 'secret_cronacle_soap_passwd' are missing."
        )

    userpasswd = b64encode(f"{soap_user}:{soap_passwd}".encode("ascii")).decode("ascii")

    headers = {
        "content-type": "application/xml; charset=utf-8",
        "authorization": f"Basic {userpasswd}",
        "SOAPAction": "/scheduler/api-soap/submit/WS_vdpreadysignaal",
    }

    body = f"""
        <soapenv:Envelope xmlns:soapenv="http://schemas.xmlsoap.org/soap/envelope/" xmlns:red="Redwood.customer.redwood.com">
            <soapenv:Header/>
                <soapenv:Body>
                    <red:SubmitJobParameters>
                        <red:omgeving>{ENV}</red:omgeving>
                    </red:SubmitJobParameters>
                </soapenv:Body>
        </soapenv:Envelope>
    """

    response = requests.post(url, data=body, headers=headers, timeout=60)
    logger.info(f"Cronacle SOAP response: {response.status_code}, {response.text}")
    response.raise_for_status()

    return {"status": response.status_code, "text": response.text}
