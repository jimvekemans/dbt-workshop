"""Tests for the between-deploy-window levers (utils/dbt_runtime_overrides.py) and the exclude list.

Run from airflow/:  python -m pytest tests   (no Airflow installation needed)
"""
import os
import stat
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from utils.dbt_command_utils import build_daily_load_command, build_exclude_args  # noqa: E402
from utils.dbt_runtime_overrides import (  # noqa: E402
    apply_overrides,
    parse_env_vars,
    parse_list,
    read_dbtignore,
    read_env_vars,
)


def test_parse_list_accepts_json_and_separated_strings():
    assert parse_list('["source:raw_carrier+", "int_web_top_pages"]') == ["source:raw_carrier+", "int_web_top_pages"]
    assert parse_list("a, b\nc  a") == ["a", "b", "c"]
    assert parse_list(["a b", ["c"]]) == ["a", "b", "c"]
    assert parse_list("") == [] and parse_list(None) == []
    with pytest.raises(ValueError):
        parse_list("[not json")


def test_exclude_list_accepts_a_json_variable():
    assert build_exclude_args(disabled_models='["source:raw_carrier+", "int_web_top_pages"]') == [
        "source:raw_carrier+", "int_web_top_pages"]
    cmd = build_daily_load_command(disabled_models='["source:raw_carrier+"]', as_bash_cmd=False)
    assert cmd[cmd.index("--exclude") + 1] == "source:raw_carrier+"


def test_env_vars_are_validated():
    assert parse_env_vars('{"DBT_VERSION_STG_CARRIER_TRACKING_EVENTS": 2, "X": true}') == {
        "DBT_VERSION_STG_CARRIER_TRACKING_EVENTS": "2", "X": "true"}
    for bad in ('{"SINGLESTORE_PASS": "x"}', '{"DBT_DEPLOY_TARGET": "LDV"}', '{"lower": "x"}', '{"A": [1]}', "[1]"):
        with pytest.raises(ValueError):
            parse_env_vars(bad)


def test_read_helpers_use_the_variables():
    values = {"dbt_env_vars": '{"DBT_SEVERITY_DEV_DATA": "error"}', "dbt_daily_load_dbtignore": '["models/*/carrier/"]'}
    get = lambda key, default=None: values.get(key, default)  # noqa: E731
    assert read_env_vars(get) == {"DBT_SEVERITY_DEV_DATA": "error"}
    assert read_dbtignore(get) == ["models/*/carrier/"]
    with pytest.raises(ValueError):
        read_dbtignore(lambda key, default=None: '["../etc/passwd"]')


@pytest.fixture
def project(tmp_path):
    """A project dir with a fake `dbt` on PATH that prints the env var and the .dbtignore it sees, then exits $RC."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    fake = bin_dir / "dbt"
    fake.write_text("#!/bin/bash\necho \"version=${DBT_VERSION_X:-unset}\"\necho \"ignore=$(tr '\\n' ' ' < .dbtignore 2>/dev/null)\"\n"
                    "exit ${RC:-0}\n")
    fake.chmod(fake.stat().st_mode | stat.S_IEXEC)
    work = tmp_path / "project"
    work.mkdir()
    return work, {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}"}


def run(cmd, cwd, env, rc=0):
    return subprocess.run(cmd, cwd=cwd, env={**env, "RC": str(rc)}, capture_output=True, text=True)


def test_overrides_are_in_place_during_the_run_and_gone_after(project):
    work, env = project
    cmd = apply_overrides(["bash", "-c", "dbt build --selector daily_load"], env_vars={"DBT_VERSION_X": "2"},
                          dbtignore=["models/*/carrier/", "tests/carrier/"])
    res = run(cmd, work, env, rc=1)
    assert res.returncode == 1                                  # dbt's exit code survives the cleanup trap
    assert "version=2" in res.stdout
    assert "ignore=models/*/carrier/ tests/carrier/" in res.stdout   # globs written literally, not expanded
    assert not (work / ".dbtignore").exists()                   # docker mode: the shared workspace is clean again


def test_the_projects_own_dbtignore_is_restored(project):
    work, env = project
    (work / ".dbtignore").write_text("scratch/\n")
    res = run(apply_overrides("dbt build", dbtignore=["models/*/carrier/"]), work, env)
    assert res.returncode == 0 and "ignore=scratch/ models/*/carrier/" in res.stdout
    assert (work / ".dbtignore").read_text() == "scratch/\n"


def test_no_overrides_leaves_the_command_alone():
    cmd = ["bash", "-c", "dbt build"]
    assert apply_overrides(cmd) is cmd
