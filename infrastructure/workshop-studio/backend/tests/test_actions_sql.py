import json

import pydantic
import pymysql
import pytest

from studio.actions import clients, sql
from studio.actions.base import ACTIONS, CLEANERS, StepFailed
from test_actions_support import make_ctx

REFUSAL = pymysql.err.OperationalError(
    1142, "DELETE command denied to user 'svc_dev_dbt'@'10.0.0.5' for table 'prd_edw_dv_bdv.dim_customer'"
)


class FakeConn:
    def __init__(self, calls, error=None, rows=()):
        self.calls, self.error, self.rows, self.description = calls, error, list(rows), None

    def cursor(self):
        return self

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def execute(self, statement):
        self.calls.append(statement)
        if self.error:
            raise self.error
        self.description = [("x",)] if self.rows else None

    def fetchmany(self, n):
        return self.rows[:n]

    def close(self):
        pass


def fake_connect(monkeypatch, calls, error=None, rows=()):
    monkeypatch.setattr(clients, "s2_connect", lambda *a, **k: FakeConn(calls, error, rows))


def test_registered_with_a_plain_label():
    p = sql.SqlExecParams.model_validate({"as": "svc_dev_dbt", "statement": "DELETE FROM x", "expect": "refused"})
    assert ACTIONS["sql.exec"].label(p) == "Nakijken of de robot van development dit mag doen"
    assert "sql" in CLEANERS


def test_change_without_undo_is_rejected_at_load_time():
    with pytest.raises(pydantic.ValidationError):
        sql.SqlExecParams.model_validate({"statement": "DELETE FROM x"})
    sql.SqlExecParams.model_validate({"statement": "SELECT 1"})
    sql.SqlExecParams.model_validate({"statement": "DELETE FROM x", "expect": "refused"})
    sql.SqlExecParams.model_validate({"statement": "DELETE FROM x", "undo": "INSERT INTO x VALUES (1)"})


def test_unknown_parameter_is_rejected():
    with pytest.raises(pydantic.ValidationError):
        sql.SqlExecParams.model_validate({"statement": "SELECT 1", "databse": "x"})


@pytest.mark.parametrize(
    "message,statement,expected",
    [
        (
            "DELETE command denied to user 'svc_dev_dbt'@'h' for table 'prd_edw_dv_bdv.t'",
            "",
            "De database weigerde: de robot van development mag de tabellen in productie niet wijzigen.",
        ),
        (
            "SELECT command denied to user 'svc_dev_dbt'@'h' for table 'prd_landing.t'",
            "",
            "De database weigerde: de robot van development mag de tabellen in productie niet lezen.",
        ),
        (
            "Access denied for user 'svc_dev_dbt'@'%' to database 'prd_edw_dv_bdv'",
            "DELETE FROM prd_edw_dv_bdv.t",
            "De database weigerde: de robot van development mag de tabellen in productie niet wijzigen.",
        ),
        (
            "Access denied for user 'svc_dev_dbt'@'%' to database 'prd_edw_dv_bdv'",
            "SELECT * FROM prd_edw_dv_bdv.t",
            "De database weigerde: de robot van development mag de tabellen in productie niet lezen.",
        ),
        (
            "CREATE command denied to user 'svc_dev_dbt'@'%' for table 'probe'",
            "CREATE TABLE prd_edw_dv_bdv.probe (a int)",
            "De database weigerde: de robot van development mag de tabellen in productie niet wijzigen.",
        ),
        ("something else", "", "De database weigerde: de robot van development mag deze opdracht niet uitvoeren."),
    ],
)
def test_refusal_is_quoted_in_plain_words(message, statement, expected):
    assert sql.describe_refusal("svc_dev_dbt", message, statement) == expected


def test_only_access_errors_count_as_refusals():
    assert sql.is_refusal(REFUSAL)
    assert not sql.is_refusal(pymysql.err.ProgrammingError(1064, "syntax error"))


def test_expected_refusal_succeeds_with_the_refusal_as_summary(tmp_path, monkeypatch):
    fake_connect(monkeypatch, [], error=REFUSAL)
    p = sql.SqlExecParams.model_validate(
        {"as": "svc_dev_dbt", "statement": "DELETE FROM prd_edw_dv_bdv.dim_customer", "expect": "refused"}
    )
    result = sql.sql_exec(make_ctx(tmp_path), p)
    assert result.summary == "De database weigerde: de robot van development mag de tabellen in productie niet wijzigen."
    assert result.touched == set()


def test_unexpected_refusal_and_unexpected_success_both_fail(tmp_path, monkeypatch):
    ok = sql.SqlExecParams.model_validate({"statement": "DELETE FROM x", "undo": "SELECT 1"})
    fake_connect(monkeypatch, [], error=REFUSAL)
    with pytest.raises(StepFailed) as err:
        sql.sql_exec(make_ctx(tmp_path), ok)
    assert err.value.summary.startswith("De database weigerde")
    fake_connect(monkeypatch, [])
    refused = sql.SqlExecParams.model_validate({"statement": "DELETE FROM x", "expect": "refused"})
    with pytest.raises(StepFailed) as err:
        sql.sql_exec(make_ctx(tmp_path), refused)
    assert "geweigerd moeten worden" in err.value.summary


def test_syntax_error_is_not_a_refusal(tmp_path, monkeypatch):
    fake_connect(monkeypatch, [], error=pymysql.err.ProgrammingError(1064, "syntax error"))
    p = sql.SqlExecParams.model_validate({"statement": "SELEKT", "expect": "refused"})
    with pytest.raises(StepFailed) as err:
        sql.sql_exec(make_ctx(tmp_path), p)
    assert err.value.summary == "De database kon de opdracht niet uitvoeren."


def test_undo_is_stored_and_run_once_by_the_cleaner(tmp_path, monkeypatch):
    calls: list[str] = []
    fake_connect(monkeypatch, calls)
    ctx = make_ctx(tmp_path)
    p = sql.SqlExecParams.model_validate({"statement": "DELETE FROM x", "undo": "INSERT INTO x VALUES (1)"})
    result = sql.sql_exec(ctx, p)
    (resource,) = result.touched
    assert resource.startswith("sql:")
    assert (
        json.loads((ctx.settings.data_dir / "sql_undo.json").read_text())[resource[4:]]["statement"]
        == "INSERT INTO x VALUES (1)"
    )
    sql.undo_sql(ctx, resource)
    sql.undo_sql(ctx, resource)  # idempotent
    assert calls == ["DELETE FROM x", "INSERT INTO x VALUES (1)"]


def test_read_returns_a_row_count(tmp_path, monkeypatch):
    fake_connect(monkeypatch, [], rows=[(1,), (2,)])
    result = sql.sql_exec(make_ctx(tmp_path), sql.SqlExecParams.model_validate({"statement": "SELECT 1"}))
    assert result.summary == "De database antwoordde met 2 rijen."


def test_undo_that_finds_nothing_left_counts_as_done(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    p = sql.SqlExecParams.model_validate({"statement": "ALTER x", "undo": "ALTER x DROP COLUMN y", "rebuild": True})
    fake_connect(monkeypatch, [])
    result = sql.sql_exec(ctx, p)
    assert any(r.startswith("sql:") for r in result.touched) and "models:dev" in result.touched
    fake_connect(monkeypatch, [], error=pymysql.err.OperationalError(1091, "Can't DROP 'y'"))
    sql.undo_sql(ctx, next(r for r in result.touched if r.startswith("sql:")))
    assert json.loads((ctx.settings.data_dir / "sql_undo.json").read_text()) == {}
