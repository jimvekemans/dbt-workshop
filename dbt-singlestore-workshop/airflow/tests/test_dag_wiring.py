"""The DAG files of dbt_daily_load, report_refreshed_notice and dbt_on_demand, run against stand-ins of the Airflow names
they use: which tasks exist, how they are wired, the schedule, and what the task callables return.

There is no Airflow on the host, and test_pod_utils.py installs its own stand-ins for the same modules, so every test
here swaps the `airflow` and `utils` modules for a fresh set and puts the old ones back afterwards.

Run from airflow/:  python -m pytest tests
"""
import importlib.util
import sys
import types
from datetime import datetime, timezone
from pathlib import Path
from types import SimpleNamespace

import pytest

AIRFLOW_DIR = Path(__file__).resolve().parents[1]
NO_NODES = "'{\"error\": [\"NoNodesForSelectionCriteria\", \"NoNodesSelected\"]}'"   # the flag's value, quoted for the shell
sys.path.insert(0, str(AIRFLOW_DIR))


# ---------------------------------------------------------------------------
# Stand-ins: just enough of airflow.sdk to build a DAG and keep what was declared
# ---------------------------------------------------------------------------


class AirflowException(Exception):
    pass


class AirflowSkipException(AirflowException):
    pass


class Variable:
    values: dict = {}

    @classmethod
    def get(cls, key, default=None):
        return cls.values.get(key, default)


class Asset:
    def __init__(self, name=None, uri=None, **kwargs):
        self.name = name or uri
        self.uri = uri or name

    def __eq__(self, other):
        return isinstance(other, Asset) and (self.name, self.uri) == (other.name, other.uri)

    def __hash__(self):
        return hash((self.name, self.uri))


class AssetAlias:
    def __init__(self, name):
        self.name = name


class Param:
    def __init__(self, default=None, **schema):
        self.default, self.schema = default, schema


class DAG:
    created: list = []
    current = None

    def __init__(self, dag_id, **kwargs):
        self.dag_id, self.kwargs = dag_id, kwargs
        self.tasks: dict = {}
        self.edges: set = set()
        DAG.created.append(self)

    def __enter__(self):
        DAG.current = self
        return self

    def __exit__(self, *exc):
        DAG.current = None
        return False

    @property
    def task_dict(self):
        return self.tasks


class TaskNode:
    def __init__(self, definition, args):
        self.definition, self.dag = definition, DAG.current
        self.task_id = definition.kwargs.get("task_id") or definition.fn.__name__
        self.dag.tasks[self.task_id] = self
        for arg in args:  # a task passed as an argument is an XCom dependency
            if isinstance(arg, TaskNode):
                self.dag.edges.add((arg.task_id, self.task_id))

    def __rshift__(self, other):
        for item in other if isinstance(other, (list, tuple)) else [other]:
            self.dag.edges.add((self.task_id, item.task_id))
        return other

    def __rrshift__(self, other):  # [a, b] >> node
        for item in other:
            self.dag.edges.add((item.task_id, self.task_id))
        return self


class TaskDefinition:
    def __init__(self, kwargs, fn):
        self.kwargs, self.fn = kwargs, fn

    def __call__(self, *args, **kwargs):
        return TaskNode(self, args)


def task(**kwargs):
    return lambda fn: TaskDefinition(kwargs, fn)


def _install_stand_ins():
    names = ["airflow", "airflow.sdk", "airflow.sdk.exceptions", "airflow.sdk.definitions", "airflow.sdk.definitions.param"]
    modules = {name: types.ModuleType(name) for name in names}
    sdk = modules["airflow.sdk"]
    sdk.DAG, sdk.Asset, sdk.AssetAlias, sdk.Param, sdk.Variable, sdk.task = DAG, Asset, AssetAlias, Param, Variable, task
    modules["airflow.sdk.exceptions"].AirflowException = AirflowException
    modules["airflow.sdk.exceptions"].AirflowSkipException = AirflowSkipException
    modules["airflow.sdk.definitions.param"].ParamsDict = dict
    sys.modules.update(modules)


def _is_ours(name: str) -> bool:
    return name in ("utils", "airflow") or name.startswith(("utils.", "airflow."))


@pytest.fixture
def stand_ins(monkeypatch):
    saved = {name: module for name, module in sys.modules.items() if _is_ours(name)}
    for name in saved:
        del sys.modules[name]
    for var in ("DBT_DAILY_LOAD_SCHEDULE", "DBT_RUNTIME", "IFW_PILLAR"):
        monkeypatch.delenv(var, raising=False)
    monkeypatch.setenv("DBT_DEPLOY_TARGET", "dev")
    _install_stand_ins()
    Variable.values = {}
    DAG.created = []
    yield
    for name in [n for n in sys.modules if _is_ours(n)]:
        del sys.modules[name]
    sys.modules.update(saved)


def load(name: str, **variables):
    """Execute dags/<name>.py with these Airflow Variables; returns (module, the DAG it declared)."""
    Variable.values = variables
    DAG.created = []
    spec = importlib.util.spec_from_file_location(f"dagfile_{name}", AIRFLOW_DIR / "dags" / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert len(DAG.created) == 1
    dag = DAG.created[0]
    dag.runs, dag.state_calls = _intercept_runner()
    return module, dag


def _intercept_runner():
    """Nothing is run: the dbt containers the Docker runner would start are recorded as (task_id, command, env), the
    state calls the python tasks would make as StateCall objects."""
    runs, state_calls = [], []
    pod = sys.modules.get("utils.dbt_pod_utils")
    if pod is None:  # a DAG without dbt tasks
        return runs, state_calls
    pod.DockerTaskRunner.run_container = lambda self, task_id, cmd, env_vars, run_id="": runs.append((task_id, cmd, env_vars))
    resolve = pod._resolve_state_call

    def spy(python_callable, context):
        state_calls.append(resolve(python_callable, context))

    pod._resolve_state_call = spy
    return runs, state_calls


def call(dag: DAG, task_id: str, *args, **context):
    return dag.tasks[task_id].definition.fn(*args, **context)


def dbt_command(dag: DAG, task_id: str, **context) -> str:
    """The shell line the dbt task of this DAG would run in its container."""
    call(dag, task_id, **context)
    run_task_id, cmd, _ = dag.runs[-1]
    assert run_task_id == task_id and cmd[:2] == ["bash", "-c"]
    return cmd[2]


FEED_A = "s2://workshop-dev/dev_landing/raw_olist_orders"
FEED_B = "s2://workshop-dev/dev_landing/raw_carrier_events"

# dbt_daily_load as it was before the asset features: what the Workshop Studio reads and the failure mail relies on
DAILY_TASKS = {"audit_environment_drift", "dbt_build", "persist_artifacts_to_state", "dbt_source_freshness",
               "send_failure_report"}
DAILY_EDGES = {
    ("audit_environment_drift", "dbt_build"), ("dbt_build", "persist_artifacts_to_state"),
    ("dbt_build", "dbt_source_freshness"), ("audit_environment_drift", "send_failure_report"),
    ("dbt_build", "send_failure_report"), ("dbt_source_freshness", "send_failure_report"),
}


# ---------------------------------------------------------------------------
# dbt_daily_load: A1 (schedule) and A2 (events), both opt-in
# ---------------------------------------------------------------------------


def test_daily_load_is_unchanged_without_the_variables(stand_ins):
    _, dag = load("dbt_daily_load")
    assert set(dag.tasks) == DAILY_TASKS and dag.edges == DAILY_EDGES
    assert dag.kwargs["schedule"] == "@daily"


def test_daily_load_keeps_its_clock_schedule_setting(stand_ins, monkeypatch):
    monkeypatch.setenv("DBT_DAILY_LOAD_SCHEDULE", "none")
    assert load("dbt_daily_load")[1].kwargs["schedule"] is None
    monkeypatch.setenv("DBT_DAILY_LOAD_SCHEDULE", "0 3 * * *")
    assert load("dbt_daily_load")[1].kwargs["schedule"] == "0 3 * * *"


def test_daily_load_starts_when_all_listed_feeds_have_new_events(stand_ins, monkeypatch):
    monkeypatch.setenv("DBT_DAILY_LOAD_SCHEDULE", "none")
    _, dag = load("dbt_daily_load", dbt_dataset_dependencies=f'["{FEED_A}", "{FEED_B}"]')
    assert dag.kwargs["schedule"] == [Asset(FEED_A), Asset(FEED_B)]       # a list: every one of them
    assert set(dag.tasks) == DAILY_TASKS and dag.edges == DAILY_EDGES       # the asset schedule adds no tasks


@pytest.mark.parametrize("value", ["[]", "", "[not json", '["no uri"]'])
def test_daily_load_falls_back_to_the_clock_when_the_variable_is_empty_or_broken(stand_ins, value):
    assert load("dbt_daily_load", dbt_dataset_dependencies=value)[1].kwargs["schedule"] == "@daily"


@pytest.mark.parametrize("value", [None, "false", "no", "", "maybe"])
def test_no_asset_tasks_unless_the_emit_variable_is_true(stand_ins, value):
    variables = {} if value is None else {"dbt_emit_asset_events": value}
    _, dag = load("dbt_daily_load", **variables)
    assert set(dag.tasks) == DAILY_TASKS and dag.edges == DAILY_EDGES


def test_emit_tasks_come_after_the_persisted_artifacts_and_join_the_failure_report(stand_ins):
    _, dag = load("dbt_daily_load", dbt_emit_asset_events="true")
    assert set(dag.tasks) == DAILY_TASKS | {"extract_refreshed_tables", "emit_asset_events"}
    assert dag.edges == DAILY_EDGES | {
        ("persist_artifacts_to_state", "extract_refreshed_tables"), ("extract_refreshed_tables", "emit_asset_events"),
        ("extract_refreshed_tables", "send_failure_report"), ("emit_asset_events", "send_failure_report")}
    emit = dag.tasks["emit_asset_events"].definition.kwargs
    assert emit["trigger_rule"] == "all_success" and [a.name for a in emit["outlets"]] == ["dbt_refreshed_tables"]


def test_extract_task_reads_this_runs_target_and_ignores_files_older_than_the_run(stand_ins):
    _, dag = load("dbt_daily_load", dbt_emit_asset_events="true")
    started = datetime(2026, 11, 21, 3, 0, tzinfo=timezone.utc)
    call(dag, "extract_refreshed_tables", dag_run=SimpleNamespace(start_date=started))
    resolved = dag.state_calls[-1]
    assert resolved.func == "extract_refreshed_tables"
    assert resolved.kwargs == {"run_results_path": "/mnt/dbt/target/run_results.json",
                               "manifest_path": "/mnt/dbt/target/manifest.json",
                               "not_before": started.timestamp() - 5}
    call(dag, "extract_refreshed_tables", dag_run=None)
    assert dag.state_calls[-1].kwargs["not_before"] is None


class _Outlets:
    """outlet_events of the Airflow context: outlet_events[alias].add(asset, extra=...)"""

    def __init__(self):
        self.added = []

    def __getitem__(self, alias):
        assert alias.name == "dbt_refreshed_tables"
        return SimpleNamespace(add=lambda asset, extra=None: self.added.append((asset.uri, extra)))


def test_emit_task_adds_one_event_per_table_and_survives_a_run_without_tables(stand_ins, monkeypatch):
    monkeypatch.delenv("SINGLESTORE_HOST", raising=False)   # no database to count rows in: the event says so
    _, dag = load("dbt_daily_load", dbt_emit_asset_events="true", dbt_image_tag="v1.0.1")
    payload = {"invocation_id": "inv-9", "tables": [
        {"database": "dev_im_dimensional", "table": "mart_olist_management_kpi", "status": "success",
         "rows_affected": 24, "execution_time": 0.8}]}
    outlets = _Outlets()
    call(dag, "emit_asset_events", payload, outlet_events=outlets)
    assert outlets.added == [("s2://workshop-dev/dev_im_dimensional/mart_olist_management_kpi", {
        "status": "success", "rows": None, "rows_affected": 24, "execution_time": 0.8, "invocation_id": "inv-9",
        "dbt_image_tag": "v1.0.1", "environment": "dev"})]
    call(dag, "emit_asset_events", {"invocation_id": "x", "tables": []}, outlet_events=outlets)   # no table: nothing, no error
    assert len(outlets.added) == 1
    with pytest.raises(AirflowException):
        call(dag, "emit_asset_events", payload, outlet_events=None)


def test_failure_report_still_reads_the_shared_target_dir(stand_ins):
    _, dag = load("dbt_daily_load")
    dag_utils = sys.modules["utils.dbt_dag_utils"]
    seen = []
    dag_utils.send_failure_report_for_run = lambda context, target_dir=None: seen.append(target_dir)
    call(dag, "send_failure_report")
    assert seen == ["/mnt/dbt/target"]


# ---------------------------------------------------------------------------
# report_refreshed_notice (A3)
# ---------------------------------------------------------------------------


def test_notice_dag_is_scheduled_on_the_management_report_table(stand_ins):
    _, dag = load("report_refreshed_notice")
    assert dag.kwargs["schedule"] == [Asset("s2://workshop-dev/dev_im_dimensional/mart_olist_management_kpi")]
    assert dag.kwargs["is_paused_upon_creation"] is True and set(dag.tasks) == {"send_refreshed_notice"}
    # the table is real: models/im_dimensional/mart_olist_management_kpi.sql
    model = AIRFLOW_DIR.parent / "models" / "im_dimensional" / "mart_olist_management_kpi.sql"
    assert model.is_file()


def test_notice_dag_sends_one_mail_with_the_event_details(stand_ins):
    module, dag = load("report_refreshed_notice")
    sent = []
    module.send_text_mail = lambda subject, body: sent.append((subject, body)) or True
    when = datetime(2026, 11, 21, 6, 3, tzinfo=timezone.utc)
    event = SimpleNamespace(timestamp=when, extra={"rows_affected": 24, "dbt_image_tag": "v1.0.1"})
    call(dag, "send_refreshed_notice", triggering_asset_events={Asset("x"): [event]})
    assert len(sent) == 1
    subject, body = sent[0]
    assert subject == "[development] Het managementrapport is ververst"
    assert "Rijen in het rapport: 24" in body and "Versie: v1.0.1" in body and "21 november, 06:03 UTC" in body


def test_notice_dag_by_hand_sends_a_test_notice_and_a_failed_mail_fails_the_task(stand_ins):
    module, dag = load("report_refreshed_notice")
    sent = []
    module.send_text_mail = lambda subject, body: sent.append(subject) or True
    call(dag, "send_refreshed_notice")                                  # no triggering events in the context
    assert "manueel gestart" in sent[0]
    module.send_text_mail = lambda subject, body: False
    with pytest.raises(AirflowException):
        call(dag, "send_refreshed_notice", triggering_asset_events={})


# ---------------------------------------------------------------------------
# dbt_on_demand (A4)
# ---------------------------------------------------------------------------

ON_DEMAND_PARAMS = {"select": "tag:on-demand", "exclude": None, "full_refresh_models": None}


def _context(params=None, conf=None, try_number=1):
    return {"task_instance": SimpleNamespace(try_number=try_number), "params": {**ON_DEMAND_PARAMS, **(params or {})},
            "dag_run": SimpleNamespace(conf=conf or {})}


def test_on_demand_dag_has_no_schedule_and_the_documented_params(stand_ins):
    _, dag = load("dbt_on_demand")
    assert dag.kwargs["schedule"] is None and dag.kwargs["is_paused_upon_creation"] is True
    params = dag.kwargs["params"]
    assert set(params) == {"select", "exclude", "full_refresh_models"}
    assert (params["select"].default, params["exclude"].default, params["full_refresh_models"].default) == (
        "tag:on-demand", None, None)
    assert set(dag.tasks) == {"dbt_build", "persist_artifacts_to_state", "send_failure_report"}
    assert dag.edges == {("dbt_build", "persist_artifacts_to_state"), ("dbt_build", "send_failure_report")}


def test_on_demand_build_command_follows_the_params_and_the_conf(stand_ins):
    _, dag = load("dbt_on_demand")
    assert dbt_command(dag, "dbt_build", **_context()) == (
        f"dbt build --select tag:on-demand --warn-error-options {NO_NODES} --target run --target-path /mnt/dbt/target_on_demand")
    assert dag.runs[-1][2]["DBT_TARGET_PATH"] == "/mnt/dbt/target_on_demand"
    line = dbt_command(dag, "dbt_build", **_context(
        params={"exclude": "tag:slow"}, conf={"select": "tag:a", "full_refresh_models": ["m1", "m2"]}))
    assert line == ("dbt build --select tag:a --exclude tag:slow --vars '{full_refresh_models: [m1, m2]}' "
                    f"--warn-error-options {NO_NODES} --target run --target-path /mnt/dbt/target_on_demand")


def test_on_demand_retries_from_its_own_state_and_refuses_an_empty_selection(stand_ins):
    _, dag = load("dbt_on_demand")
    assert dbt_command(dag, "dbt_build", **_context(try_number=2)) == (
        "dbt retry --state /mnt/dbt/artifacts/on_demand --target run --target-path /mnt/dbt/target_on_demand")
    with pytest.raises(ValueError):
        call(dag, "dbt_build", **_context(conf={"select": " "}))


def test_on_demand_runner_environment_variables_apply_and_state_stays_out_of_the_daily_folders(stand_ins):
    _, dag = load("dbt_on_demand", dbt_env_vars='{"DBT_TEST_SEVERITY": "error"}')
    assert dbt_command(dag, "dbt_build", **_context()).startswith("export DBT_TEST_SEVERITY=error; dbt build ")
    persist = dbt_command(dag, "persist_artifacts_to_state")
    assert "/mnt/dbt/target_on_demand/." in persist and "/mnt/dbt/artifacts/on_demand/" in persist
    assert "daily_load" not in persist and "/mnt/dbt/target/" not in persist


def test_on_demand_failure_report_reads_its_own_target_dir(stand_ins):
    _, dag = load("dbt_on_demand")
    seen = []
    sys.modules["utils.dbt_dag_utils"].send_failure_report_for_run = lambda context, target_dir=None: seen.append(target_dir)
    call(dag, "send_failure_report")
    assert seen == ["/mnt/dbt/target_on_demand"]
