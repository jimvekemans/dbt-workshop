"""Single import point for Airflow 3.x (airflow.sdk) and Airflow 2.x (2.11) APIs."""

try:  # Airflow 3.x
    from airflow.sdk import DAG, Param, TaskGroup, Variable, task
    from airflow.sdk import Asset as Dataset, AssetAlias as DatasetAlias
    from airflow.sdk.definitions.param import ParamsDict
    from airflow.sdk.exceptions import AirflowException, AirflowSkipException
    from airflow.providers.standard.operators.trigger_dagrun import TriggerDagRunOperator
except ImportError:  # Airflow 2.x
    from airflow import DAG
    from airflow.datasets import Dataset, DatasetAlias
    from airflow.decorators import task
    from airflow.exceptions import AirflowException, AirflowSkipException
    from airflow.models import Variable
    from airflow.models.param import Param, ParamsDict
    from airflow.operators.trigger_dagrun import TriggerDagRunOperator
    from airflow.utils.task_group import TaskGroup


def get_variable(key, default=None):
    """Variable.get with a default: `default` on Airflow 3, `default_var` on Airflow 2."""
    try:
        return Variable.get(key, default=default)
    except TypeError:
        return Variable.get(key, default_var=default)


__all__ = [
    "AirflowException",
    "AirflowSkipException",
    "DAG",
    "Dataset",
    "DatasetAlias",
    "Param",
    "ParamsDict",
    "TaskGroup",
    "TriggerDagRunOperator",
    "Variable",
    "get_variable",
    "task",
]
