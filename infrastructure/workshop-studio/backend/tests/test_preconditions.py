"""Scenario order safety: `requires:` checks stop a live injection before anything runs, tag waits fail fast when CI
tagged the head with something else, and Cancel stops a long live step. Plus the Domain A patch applying on any dev."""
import shutil
import threading
import time
from pathlib import Path

import pytest

from app import actions
from app.actions import gitlab as gitlab_mod
from app.actions.base import Action, ActionError, Cancelled, Ctx, PreconditionFailed
from app.patches import PatchError, Patch, apply_to_tree, load_patch, resolve_contents
from app.schema import Scenario

ROOT = Path(__file__).resolve().parents[4]   # repo root: infrastructure/workshop-studio/backend/tests/..
FCT = "models/im_dimensional/fct_olist_orders.sql"


# ----------------------------------------------------------------------------------------------- content
STARTING_STATE = {   # scenario -> checks it must declare (the live run depends on them)
    "H0_happy_sprint": {"check_gitlab_baseline"},
    "E1_release_collision": {"check_gitlab_baseline", "check_sql"},
    "C1_finance_fanout": {"check_gitlab_file"},
    "S3_drive_duplicate": {"check_gitlab_file"},
}


@pytest.mark.parametrize("sid", sorted(STARTING_STATE))
def test_star_scenarios_declare_their_starting_state(engine, sid):
    sc = engine.scenarios[sid]
    assert STARTING_STATE[sid] <= {a.action for a in sc.requires}
    assert all(a.params().get("hint") for a in sc.requires), "every requirement tells the presenter what to do"
    assert next(s for s in engine.catalogue() if s["id"] == sid)["requires"]


def test_s3_runs_dbt_before_its_detect_step(engine):
    """The unique test can only fail if dbt runs after the duplicates landed (C1 does the same in its injection)."""
    acts = [a.action for a in engine.scenarios["S3_drive_duplicate"].inject.live]
    assert acts.index("airflow_trigger") > acts.index("trickle")


def test_requires_accepts_only_read_only_checks():
    with pytest.raises(ValueError, match="check_"):
        Scenario.model_validate({"id": "X", "layer": "all", "title": "t", "inject": {"narrative": "n"},
                                 "requires": [{"action": "reset"}]})


# ------------------------------------------------------------------------------------------ engine order
def _spy_scenario(engine, monkeypatch, check_ok: bool):
    ran: list[str] = []

    class FakeCheck(Action):
        name = "check_fake"

        def render(self, p, ctx):
            return ["# check: fake"]

        def run(self, p, ctx):
            if not check_ok:
                raise PreconditionFailed(p["hint"] + " (dev has moved past v1.0.0)")
            return actions.Result(output="ok")

    class Spy(Action):
        name = "spy"

        def render(self, p, ctx):
            return ["spy"]

        def run(self, p, ctx):
            ran.append("spy")
            return actions.Result(output="ran")

    monkeypatch.setitem(actions.REGISTRY, "check_fake", FakeCheck())
    monkeypatch.setitem(actions.REGISTRY, "spy", Spy())
    sc = Scenario.model_validate({
        "id": "T", "layer": "all", "title": "t",
        "requires": [{"action": "check_fake", "hint": "press Reset first"}, {"action": "check_fake", "hint": "second"}],
        "inject": {"narrative": "n", "live": [{"action": "spy"}], "sim": {"git.main": {"health": "fail"}}},
    })
    engine.scenarios[sc.id] = sc
    return ran


def test_unmet_requirement_stops_live_injection_before_anything_runs(engine, monkeypatch):
    ran = _spy_scenario(engine, monkeypatch, check_ok=False)
    engine.set_mode("live")
    engine.set_dry_run(False)
    before = engine.snapshot()
    with pytest.raises(PreconditionFailed) as err:
        engine.inject("T")
    assert "press Reset first" in str(err.value) and "second" in str(err.value)   # all unmet checks reported at once
    assert ran == []
    snap = engine.snapshot()
    assert snap["scenarioId"] == before["scenarioId"] and snap["nodes"]["git.main"]["health"] == before["nodes"]["git.main"]["health"]
    assert snap["busy"] is None and "press Reset first" in snap["error"]


def test_requirements_pass_then_inject_runs(engine, monkeypatch):
    ran = _spy_scenario(engine, monkeypatch, check_ok=True)
    engine.set_mode("live")
    engine.set_dry_run(False)
    assert engine.inject("T")["scenarioId"] == "T" and ran == ["spy"]


def test_requirements_are_not_checked_in_sim(engine, monkeypatch):
    ran = _spy_scenario(engine, monkeypatch, check_ok=False)
    assert engine.inject("T")["scenarioId"] == "T" and ran == []   # sim needs nothing from the stack


def test_unmet_requirement_is_http_412(settings, monkeypatch):
    from fastapi.testclient import TestClient

    from app.main import create_app
    app = create_app(settings, start_probes=False)
    _spy_scenario(app.state.engine, monkeypatch, check_ok=False)
    with TestClient(app, headers={"X-Studio": "1"}) as c:
        c.post("/api/mode", json={"mode": "live"})
        c.post("/api/dry-run", json={"dry_run": False})
        r = c.post("/api/scenarios/T/inject")
        assert r.status_code == 412 and "press Reset first" in r.json()["detail"]


# ------------------------------------------------------------------------------------------------ cancel
def test_cancel_stops_a_waiting_step_and_leaves_it_pending(engine, monkeypatch):
    class Wait(Action):
        name = "wait_forever"

        def render(self, p, ctx):
            return ["wait"]

        def run(self, p, ctx):
            deadline = time.time() + 20
            while time.time() < deadline:
                ctx.sleep(5)          # like a CI poll: cancel must cut the sleep short
            return actions.Result(output="done")

    monkeypatch.setitem(actions.REGISTRY, "wait_forever", Wait())
    engine.scenarios["W"] = Scenario.model_validate({
        "id": "W", "layer": "all", "title": "t", "inject": {"narrative": "n"},
        "steps": [{"title": "s", "live": [{"action": "wait_forever"}], "sim": {"git.main": {"health": "fail"}}}]})
    engine.set_mode("live")
    engine.set_dry_run(False)
    engine.inject("W")
    with pytest.raises(ValueError, match="nothing is running"):
        engine.cancel()

    errors: list[BaseException] = []

    def run():
        try:
            engine.run_step()
        except BaseException as e:  # noqa: BLE001
            errors.append(e)

    t = threading.Thread(target=run)
    started = time.time()
    t.start()
    while engine.state["busy"] is None:
        time.sleep(0.01)
    engine.cancel()
    t.join(timeout=5)
    assert not t.is_alive() and time.time() - started < 5
    assert len(errors) == 1 and isinstance(errors[0], Cancelled)
    snap = engine.snapshot()
    assert snap["stepIndex"] == 0 and snap["busy"] is None and "cancelled" in snap["error"]
    assert snap["nodes"]["git.main"]["health"] != "fail"      # map unchanged
    assert not engine._cancel.is_set()                         # the next operation starts clean


# ------------------------------------------------------------------------------------- tag wait fail-fast
def test_unexpected_head_tag():
    from app.actions.gitlab import unexpected_head_tag
    assert unexpected_head_tag("v1.1.0", ["v1.2.0"]) == "v1.2.0"            # dev already held other work
    assert unexpected_head_tag("v1.1.0", ["v1.0.0"]) is None                # head not moved yet: keep waiting
    assert unexpected_head_tag("v1.1.0", []) is None
    assert unexpected_head_tag("v1.1.0", ["v1.1.0"]) is None
    assert unexpected_head_tag("2026-Q1.0", ["2026-Q1.1"]) == "2026-Q1.1"   # main was promoted before
    assert unexpected_head_tag("2026-Q1.0", ["2026_DROP1.0"]) is None       # baseline: keep waiting


class _FakeResp:
    def __init__(self, status, data):
        self.status_code, self._data = status, data

    def json(self):
        return self._data


def _fake_gitlab(tags: dict[str, str], branches: dict[str, str], head_refs: dict[str, list[str]], all_tags=None):
    class FakeGL:
        def __init__(self, ctx):
            pass

        def p(self, path):
            return path

        def branch_sha(self, name):
            return branches[name]

        def tag_sha(self, tag):
            return tags.get(tag)

        def tag_names(self):
            return list(all_tags if all_tags is not None else tags)

        def req(self, method, path, ok=(200,), **kw):
            if path.startswith("/repository/tags/"):
                name = path.rsplit("/", 1)[1]
                return _FakeResp(200, {"commit": {"id": tags[name]}}) if name in tags else _FakeResp(404, {})
            if path.startswith("/repository/commits/"):
                sha = path.split("/")[3]
                return _FakeResp(200, [{"name": n} for n in head_refs.get(sha, [])])
            raise AssertionError(path)

    return FakeGL


def test_tag_wait_fails_fast_when_ci_tagged_the_head_differently(settings, monkeypatch):
    monkeypatch.setattr(gitlab_mod, "GitLab", _fake_gitlab(
        tags={"v1.1.0": "old", "v1.2.0": "head"}, branches={"dev": "head"}, head_refs={"head": ["v1.2.0"]}))
    ctx = Ctx(settings=settings, log=lambda _l: None)
    started = time.time()
    with pytest.raises(ActionError, match="tagged v1.2.0, not v1.1.0.*Reset"):
        gitlab_mod.GitlabTagWait().run({"tag": "v1.1.0", "ref": "dev", "timeout": 30}, ctx)
    assert time.time() - started < 2


def test_tag_wait_still_succeeds_on_the_head(settings, monkeypatch):
    monkeypatch.setattr(gitlab_mod, "GitLab", _fake_gitlab(
        tags={"v1.1.0": "head"}, branches={"dev": "head"}, head_refs={"head": ["v1.1.0"]}))
    res = gitlab_mod.GitlabTagWait().run({"tag": "v1.1.0", "ref": "dev"}, Ctx(settings=settings, log=lambda _l: None))
    assert "exists" in res.output


def test_file_check_not_contains_ignores_a_missing_file(settings, monkeypatch):
    """Live finding: S3's 'dedup fix not in dev' check must not fail (with misleading advice) when the file is absent."""
    from app.actions import checks

    class FakeGL:
        def __init__(self, ctx):
            pass

        def read_file(self, path, ref):
            return {"present.sql": "select 1", "fixed.sql": "row_number() over ()"}.get(path)

    monkeypatch.setattr(checks, "GitLab", FakeGL)
    ctx = Ctx(settings=settings, log=lambda _l: None)
    chk = checks.CheckGitlabFile()
    chk.run({"path": "missing.sql", "not_contains": "row_number"}, ctx)               # absent: fine
    chk.run({"path": "present.sql", "not_contains": "row_number"}, ctx)               # present, clean: fine
    with pytest.raises(PreconditionFailed, match="already contains"):
        chk.run({"path": "fixed.sql", "not_contains": "row_number"}, ctx)
    with pytest.raises(PreconditionFailed, match="is not on dev"):
        chk.run({"path": "missing.sql"}, ctx)                                          # plain existence check
    with pytest.raises(PreconditionFailed, match="is not on dev"):
        chk.run({"path": "missing.sql", "exists": True, "not_contains": "row_number"}, ctx)


def test_baseline_check(settings, monkeypatch):
    from app.actions import checks
    ctx = Ctx(settings=settings, log=lambda _l: None)
    clean = {"v1.0.0": "base", "2026_DROP1.0": "base"}
    monkeypatch.setattr(checks, "GitLab", _fake_gitlab(clean, {"dev": "base", "main": "base"}, {}))
    checks.CheckGitlabBaseline().run({}, ctx)
    monkeypatch.setattr(checks, "GitLab", _fake_gitlab({**clean, "v1.1.0": "x"}, {"dev": "x", "main": "base"}, {}))
    with pytest.raises(PreconditionFailed, match="press Reset.*dev has moved past v1.0.0.*v1.1.0"):
        checks.CheckGitlabBaseline().run({"hint": "press Reset"}, ctx)


# --------------------------------------------------------------------------------------- Domain A patch
@pytest.fixture()
def baseline_tree(tmp_path):
    tree = tmp_path / "repo"
    shutil.copytree(ROOT / "dbt-singlestore-workshop", tree,
                    ignore=shutil.ignore_patterns(".git", "target", "dbt_packages", "logs", "airflow"))
    return tree


def test_domain_a_alone_adds_only_its_own_column(baseline_tree, settings):
    """H0 merges Domain A into a clean dev: fct_olist_orders must not pick up Domain B's column."""
    apply_to_tree(load_patch(settings.patches_dir, "feature-domain-a-payments"), baseline_tree)
    fct = (baseline_tree / FCT).read_text()
    assert fct.count("domain_a_hkey") == 1 and "domain_b_hkey" not in fct


def test_domain_a_on_top_of_b_lands_on_the_adjacent_line(baseline_tree, settings):
    """E1: B first, then A - the hkeys end up on adjacent lines, which is what makes the revert of B conflict."""
    apply_to_tree(load_patch(settings.patches_dir, "feature-domain-b-reviews"), baseline_tree)
    apply_to_tree(load_patch(settings.patches_dir, "feature-domain-a-payments"), baseline_tree)
    lines = (baseline_tree / FCT).read_text().splitlines()
    a = next(i for i, l in enumerate(lines) if "domain_a_hkey" in l)
    assert "domain_b_hkey" in lines[a + 1]


def test_find_replace_anchor_must_be_unique():
    patch = Patch(name="p", title="t", message="m", branch="b", ops=[{"path": "f", "find": "x", "replace": "y"}])
    with pytest.raises(PatchError, match="2 times"):
        resolve_contents(patch, lambda _p: "x x")
    with pytest.raises(PatchError, match="not found"):
        resolve_contents(patch, lambda _p: "z")
