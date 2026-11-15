import sys

import pytest

from studio.actions import clients
from studio.actions.base import Cancelled, StepFailed
from test_actions_support import make_ctx, make_settings


def test_passwords_are_parsed_and_env_placeholder_expanded(tmp_path):
    s = make_settings(tmp_path)
    assert clients.password_for(s, "pu_dev1") == "dev-pw"
    assert clients.password_for(s, "svc_prd_dbt") == "svc-pw"
    assert clients.password_for(s, "svc_val_dbt") == "svc-pw"
    assert clients.password_for(s, "root") == "root-pw"


def test_unknown_user_is_a_plain_failure(tmp_path):
    with pytest.raises(StepFailed) as err:
        clients.password_for(make_settings(tmp_path), "nobody")
    assert "nobody" in err.value.summary


def test_mask_hides_every_known_secret(tmp_path):
    s = make_settings(tmp_path)
    assert clients.mask(s, "token glpat-secret and dev-pw and root-pw") == "token *** and *** and ***"


def test_run_masks_output(tmp_path):
    ctx = make_ctx(tmp_path)
    proc = clients.run(ctx, [sys.executable, "-c", "print('pw=dev-pw')"])
    assert proc.returncode == 0 and "dev-pw" not in proc.stdout and "***" in proc.stdout


def test_cancel_stops_a_running_command(tmp_path):
    ctx = make_ctx(tmp_path)
    ctx.cancel.set()
    with pytest.raises(Cancelled):
        clients.run(ctx, [sys.executable, "-c", "import time; time.sleep(30)"])


def test_timeout_fails_the_step(tmp_path):
    with pytest.raises(StepFailed):
        clients.run(make_ctx(tmp_path), [sys.executable, "-c", "import time; time.sleep(30)"], timeout=0.6)


def test_read_rows_refuses_anything_but_a_single_read(tmp_path):
    s = make_settings(tmp_path)
    for query in ("DROP TABLE x", "SELECT 1; DROP TABLE x", "DELETE FROM x"):
        with pytest.raises(StepFailed):
            clients.read_rows(s, query)


def test_deployed_tag_reads_the_airflow_setting(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    monkeypatch.setattr(clients, "airflow_cli", lambda *a, **k: clients.Proc(0, "v1.2.0\n", ""))
    assert clients.deployed_tag(ctx, "dev") == "v1.2.0"
    monkeypatch.setattr(clients, "airflow_cli", lambda *a, **k: clients.Proc(1, "", "Variable does not exist"))
    with pytest.raises(StepFailed):
        clients.deployed_tag(ctx, "dev")
