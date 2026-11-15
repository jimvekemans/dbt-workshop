"""Shared fakes for the tests of the real actions (no test lives here)."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

from studio.actions import clients
from studio.actions.base import Ctx

SQL_FILES = {
    "init_global.sql": "CREATE USER IF NOT EXISTS 'svc_ci_dbt'@'%' IDENTIFIED BY 'ci-pw';\n"
    "CREATE USER IF NOT EXISTS 'pu_dev1'@'%' IDENTIFIED BY 'dev-pw';\n",
    "init_env.sql.tmpl": "CREATE USER IF NOT EXISTS 'svc___ENV___dbt'@'%' IDENTIFIED BY 'svc-pw';\n",
}


class FakeFeeder:
    def today(self, env: str):
        from datetime import date

        return date(2026, 3, 1)


def make_settings(tmp_path: Path) -> SimpleNamespace:
    sql_dir = tmp_path / "sql"
    sql_dir.mkdir(exist_ok=True)
    for name, text in SQL_FILES.items():
        (sql_dir / name).write_text(text)
    clients._parse_passwords.cache_clear()
    return SimpleNamespace(
        s2_host="singlestore",
        s2_port=3306,
        s2_root_pwd="root-pw",
        mysql_host="mysql",
        gitlab_url="http://gitlab.local",
        gitlab_pat="glpat-secret",
        mailpit_url="http://mailpit:8025",
        docs_host="workshop-dbt-docs",
        airflow_container_prefix="workshop-airflow-",
        docker_network="workshop_net",
        baseline_tag="v1.0.0",
        sql_dir=sql_dir,
        state_dir=tmp_path / "state",
        data_dir=tmp_path / "data",
        patches_dir=tmp_path / "patches",
        dbt_project_dir=tmp_path / "project",
        overview_url="http://localhost:8099",
    )


def make_ctx(tmp_path: Path, env: str = "dev") -> Ctx:
    ctx = Ctx(env=env, settings=make_settings(tmp_path), feeder=FakeFeeder())
    ctx.messages = []  # type: ignore[attr-defined]
    ctx.progress = ctx.messages.append  # type: ignore[method-assign]
    ctx.sleep = lambda seconds: None  # type: ignore[method-assign]
    return ctx
