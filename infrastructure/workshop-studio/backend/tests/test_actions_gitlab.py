import pydantic
import pytest

from studio.actions import clients, git_scripts, gitlab
from studio.actions.base import StepFailed
from studio.patches import Patch, PatchError, load_patch, resolve_contents
from test_actions_support import make_ctx


class FakeApi:
    """Just enough of GitLabApi: branches, tags, merge requests, pipelines and jobs held in dicts."""

    def __init__(self):
        self.branches = {"dev": "d0", "main": "m0", "feature/a": "f0"}
        self.tags = {"v1.0.0": "d0"}
        self.mrs: list[dict] = []
        self.pipelines: dict[tuple, dict] = {}
        self.job_list: dict[int, list[dict]] = {}
        self.calls: list[tuple] = []
        self.commits = {"d0", "m0"}

    def maybe(self, path):
        return {"id": path.rsplit("/", 1)[-1]} if path.rsplit("/", 1)[-1] in self.commits else None

    def branch_sha(self, name):
        return self.branches.get(name)

    def tag_sha(self, name):
        return self.tags.get(name)

    def branch_names(self):
        return list(self.branches)

    def tag_names(self):
        return list(self.tags)

    def delete_branch(self, name):
        self.calls.append(("delete_branch", name))
        self.branches.pop(name, None)

    def pipeline_for(self, sha, ref=None):
        return self.pipelines.get((sha, ref))

    def jobs(self, pipeline_id):
        return self.job_list.get(pipeline_id, [])

    def p(self, path):
        return path

    def get(self, path, **params):
        if path == "/merge_requests":
            return [
                m for m in self.mrs if all(m.get(k) == v for k, v in params.items() if k in ("state", "source_branch"))
            ]
        return next(m for m in self.mrs if path.endswith(str(m["iid"])))

    def req(self, method, path, **kw):
        self.calls.append((method, path, kw.get("json") or kw.get("params")))
        return type("R", (), {"status_code": 200, "json": lambda self: {}})()

    def failed_job_tail(self, pid):
        return ""


def test_demo_branch_names_are_enforced():
    gitlab.BranchParams.model_validate({"name": "demo/x", "from": "dev"})
    gitlab.CommitParams(branch="demo/x", patch="p")
    for bad in ({"name": "dev"}, {"name": "feature/x"}):
        with pytest.raises(pydantic.ValidationError):
            gitlab.BranchParams.model_validate(bad)
    with pytest.raises(pydantic.ValidationError):
        gitlab.CommitParams(branch="main", patch="p")


def test_pipeline_targets_are_validated():
    for ok in ("mr:demo/x", "ref:dev", "tag:v1\\.\\d+\\.0"):
        assert gitlab.WaitPipelineParams.model_validate({"for": ok}).for_ == ok
    with pytest.raises(pydantic.ValidationError):
        gitlab.WaitPipelineParams.model_validate({"for": "dev"})
    with pytest.raises(pydantic.ValidationError):
        gitlab.PlayJobParams.model_validate({"job": "deploy_prd", "for": "x:y"})


def test_manual_pipelines_count_as_finished():
    api = FakeApi()
    api.job_list[1] = [{"status": "success"}, {"status": "manual"}]
    assert gitlab.settled(api, {"id": 1, "status": "manual"}) == "success"
    api.job_list[1] = [{"status": "failed"}, {"status": "manual"}]
    assert gitlab.settled(api, {"id": 1, "status": "manual"}) == "failed"
    assert gitlab.settled(api, {"id": 1, "status": "running"}) == "running"


def test_find_pipeline_by_ref_and_by_new_tag_only():
    api = FakeApi()
    api.pipelines[("d0", "dev")] = {"id": 1}
    api.pipelines[("d0", "v1.0.0")] = {"id": 2}
    assert gitlab.find_pipeline(api, "ref:dev", []) == {"id": 1}
    assert gitlab.find_pipeline(api, "tag:v\\d+\\.\\d+\\.\\d+", ["v1.0.0"]) is None  # the baseline tag is not news
    api.tags["v1.10.0"], api.tags["v1.2.0"] = "d9", "d8"
    api.pipelines[("d9", "v1.10.0")] = {"id": 3}
    assert gitlab.find_pipeline(api, "tag:v\\d+\\.\\d+\\.\\d+", ["v1.0.0"]) == {"id": 3}  # newest by number


def test_wait_pipeline_outcomes(tmp_path, monkeypatch):
    api = FakeApi()
    monkeypatch.setattr(gitlab, "GitLabApi", lambda s: api)
    monkeypatch.setattr(gitlab, "snapshot", lambda s, a: {"tags": []})
    api.pipelines[("d0", "dev")] = {"id": 1, "status": "failed", "ref": "dev"}
    api.job_list[1] = [{"name": "verify_feature_to_dev", "status": "failed"}]
    ctx = make_ctx(tmp_path)
    ok = gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "ref:dev", "expect": "failed"}))
    assert ok.summary == "The automatic checks failed, as expected: trying the change in a private copy."
    with pytest.raises(StepFailed) as err:
        gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "ref:dev"}))
    assert err.value.summary == "The automatic checks failed at: trying the change in a private copy."
    api.pipelines[("d0", "dev")] = {"id": 1, "status": "success", "ref": "dev"}
    with pytest.raises(StepFailed):
        gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "ref:dev", "expect": "failed"}))


def test_what_a_merge_or_job_changes():
    assert gitlab.versions_touched("dev") == {"version:dev"} and gitlab.versions_touched("main") == {"version:val"}
    assert gitlab.versions_touched("release/2026-Q1") == set()
    assert gitlab.job_versions("deploy_prd") == {"version:prd"} and gitlab.job_versions("tag_main") == {"version:val"}


def test_snapshot_is_recorded_once_and_renewed_when_stale(tmp_path):
    ctx, api = make_ctx(tmp_path), FakeApi()
    first = gitlab.snapshot(ctx.settings, api)
    assert first == {"dev": "d0", "main": "m0", "branches": ["dev", "main", "feature/a"], "tags": ["v1.0.0"]}
    assert {"studio-baseline/dev", "studio-baseline/main"} <= {c[2]["branch"] for c in api.calls if c[0] == "POST"}
    api.branches["dev"] = "d1"
    assert gitlab.snapshot(ctx.settings, api)["dev"] == "d0"  # remembered, not re-read
    api.commits.clear()
    assert gitlab.snapshot(ctx.settings, api)["dev"] == "d1"  # its commits are gone: GitLab was rebuilt
    assert gitlab.recorded_tags(ctx.settings) == ["v1.0.0"]


def test_cleaner_removes_only_what_the_demos_left(tmp_path, monkeypatch):
    ctx, api = make_ctx(tmp_path), FakeApi()
    monkeypatch.setattr(git_scripts, "connect", lambda c: (api, gitlab.snapshot(c.settings, api)))
    monkeypatch.setattr(git_scripts, "move_back", lambda c, a, moved: api.calls.append(("move_back", moved)))
    removed_images = []
    monkeypatch.setattr(git_scripts.clients, "docker", lambda c, *a, **k: removed_images.append(a[1]))
    gitlab.snapshot(ctx.settings, api)
    api.branches.update({"demo/x": "x1", "release/2026-Q1": "r1", "dev": "d5"})
    api.tags.update({"v1.1.0": "d5", "2026-Q1.0": "m5"})
    api.mrs = [
        {"iid": 7, "source_branch": "demo/x", "state": "opened"},
        {"iid": 8, "source_branch": "feature/a", "state": "opened"},
    ]
    git_scripts.clean_git(ctx, "git")
    names = [c for c in api.calls if c[0] in ("delete_branch", "move_back") or c[0] == "DELETE"]
    assert ("delete_branch", "demo/x") in names and ("delete_branch", "release/2026-Q1") in names
    assert ("delete_branch", "feature/a") not in names and ("delete_branch", "dev") not in names
    assert {c[1] for c in api.calls if c[0] == "DELETE"} == {"/repository/tags/v1.1.0", "/repository/tags/2026-Q1.0"}
    assert ("move_back", {"dev": "d0"}) in names
    assert sorted(removed_images) == ["dbt-vdp:2026-Q1.0", "dbt-vdp:v1.1.0"]
    assert [c[2] for c in api.calls if c[0] == "PUT"] == [{"state_event": "close"}]


def test_cleaner_does_nothing_when_nothing_moved(tmp_path, monkeypatch):
    ctx, api = make_ctx(tmp_path), FakeApi()
    monkeypatch.setattr(git_scripts, "connect", lambda c: (api, gitlab.snapshot(c.settings, api)))
    monkeypatch.setattr(git_scripts, "move_back", lambda *a: pytest.fail("must not rewrite branches"))
    assert git_scripts.clean_git(ctx, "git").details.endswith("moved back: nothing")


def test_git_scripts_params():
    git_scripts.RevertParams(branch="release/2026-Q1", tag="v1.2.0", resolution="fix")
    git_scripts.PromoteParams(branch="release/2026-Q1")
    with pytest.raises(pydantic.ValidationError):
        git_scripts.PromoteParams(branch="x", tag="y")


def test_remote_url_embeds_the_token_only_in_the_clone_url(tmp_path):
    assert (
        git_scripts.remote_url(make_ctx(tmp_path).settings)
        == "http://root:glpat-secret@gitlab.local/workshop/dbt-vdp.git"
    )


# --- patches ---------------------------------------------------------------------------------------------------------


def write_patch(root, name, meta, files):
    d = root / name
    d.mkdir(parents=True)
    (d / "_patch.yaml").write_text(meta)
    for rel, text in files.items():
        (d / rel).parent.mkdir(parents=True, exist_ok=True)
        (d / rel).write_text(text)


def test_patch_with_full_files_and_find_replace(tmp_path):
    write_patch(
        tmp_path,
        "p",
        "title: Fix\nmessage: Fix the thing\nops:\n  - {path: a.sql, find: 'x = 1', replace: 'x = 2'}\n",
        {"new/b.sql": "select 1;"},
    )
    patch = load_patch(tmp_path, "p")
    assert patch.title == "Fix" and patch.message == "Fix the thing" and patch.touched_paths() == ["a.sql", "new/b.sql"]
    out = resolve_contents(patch, lambda path: "select x = 1 from t" if path == "a.sql" else None)
    assert out == {"new/b.sql": "select 1;", "a.sql": "select x = 2 from t"}


@pytest.mark.parametrize(
    "current,fragment", [("nothing here", "not found"), ("x = 1 and x = 1", "2 times"), (None, "does not exist")]
)
def test_find_replace_must_match_exactly_once(current, fragment):
    patch = Patch("p", "t", "m", ops=[{"path": "a.sql", "find": "x = 1", "replace": "x = 2"}])
    with pytest.raises(PatchError) as err:
        resolve_contents(patch, lambda path: current)
    assert fragment in err.value.details and "does not fit" in err.value.summary


def test_unknown_patch_is_a_plain_failure(tmp_path):
    with pytest.raises(StepFailed):
        load_patch(tmp_path, "missing")


def test_pipeline_of_an_older_commit_is_not_the_current_one():
    assert gitlab.is_current({"sha": "a"}, {"sha": "a"}) and gitlab.is_current({"id": 1}, {"sha": "a"})
    assert not gitlab.is_current({"sha": "old"}, {"sha": "new"})


def test_release_contents_names_what_was_taken_out(tmp_path, monkeypatch):
    b, a = "b" * 40, "a" * 40
    log = (
        f"{'c' * 40}\x1fRevert \"Merge branch 'demo/feature/team-reviews'\"\x1fThis reverts commit {b}, reversing changes\x1e\n"
        f"{b}\x1fMerge branch 'demo/feature/team-reviews' into 'dev'\x1f\nfeat: review scores overview\n\nSee merge request !7\x1e\n"
        f"{a}\x1fMerge branch 'demo/feature/team-payments' into 'dev'\x1f\nfeat: payments overview\n\nSee merge request !6\x1e"
    )
    tags = {b: "v1.2.0\nsomething-else", a: "v1.1.0"}

    def fake_git(ctx, repo, *args, **k):
        return clients.Proc(0, tags.get(args[-1], "") if args[0] == "tag" else log, "")

    monkeypatch.setattr(git_scripts, "git", fake_git)
    taken_out, still_in = git_scripts.release_contents(make_ctx(tmp_path), "repo")
    assert (taken_out, still_in) == (["the reviews team's change (v1.2.0), review scores overview"],
                                     ["the payments team's change (v1.1.0), payments overview"])


def test_a_change_without_a_team_branch_or_number_is_named_by_its_title():
    assert git_scripts.change_words("tidy up", "demo/feature/b", None) == "tidy up"
    assert git_scripts.change_words("tidy up", "demo/feature/b", "v1.3.0") == "tidy up (v1.3.0)"


# --- what a rehearsal covered, and who approved -------------------------------------------------------------------

REHEARSAL_LOG = """\
$ dbt compile --target-path artifacts --target run
==================== Execution Summary =====================
Finished 'compile' successfully for target 'run' [2.1s]
Processed: 1 hook | 28 models | 21 tests | 14 sources
$ dbt build --select state:modified+ --state artifacts --defer --target ci
Succeeded model demo_dim_olist_customers (table) [1 of 4 in 0.4s]
Succeeded test unique_dim_olist_customers_customer_hkey [1 of 3 in 0.1s]

==================== Execution Summary =====================
Finished 'build' successfully for target 'ci' [1.9s]
Processed: 1 hook | 4 models | 3 tests
Summary: 8 total | 8 success
"""


def test_the_closing_line_of_the_build_gives_the_counts_not_the_compile_before_it():
    assert gitlab.nodes_in(REHEARSAL_LOG) == {"tables": 4, "checks": 3}
    assert gitlab.nodes_in("Finished 'build' successfully\nProcessed: 1 hook | 1 model\n") == {"tables": 1, "checks": 0}
    assert gitlab.nodes_in("Finished 'build' with errors\nProcessed: 2 models | 1 unit test | 2 tests\n") == {
        "tables": 2, "checks": 3}


def test_without_a_closing_line_the_result_lines_are_counted():
    log = "Succeeded model a (table)\nFailed test b\nSkipped model c\nSucceeded test d\nPassed unit_test e\n"
    assert gitlab.nodes_in(log) == {"tables": 1, "checks": 3}
    assert gitlab.nodes_in("nothing useful") == {"tables": 0, "checks": 0}


def with_manifest(ctx, models=28):
    path = ctx.settings.state_dir / "dev" / "mnt_dbt" / "target"
    path.mkdir(parents=True)
    nodes = {f"model.p.m{i}": {"resource_type": "model"} for i in range(models)}
    nodes["test.p.t"] = {"resource_type": "test"}
    (path / "manifest.json").write_text(__import__("json").dumps({"nodes": nodes}))


def test_the_total_comes_from_the_manifest_of_the_deployed_project(tmp_path):
    ctx = make_ctx(tmp_path)
    assert gitlab.tables_in_project(ctx.settings) is None   # no manifest yet
    with_manifest(ctx)
    assert gitlab.tables_in_project(ctx.settings) == 28


def test_a_night_that_left_tables_out_does_not_shrink_the_project(tmp_path):
    import json
    ctx = make_ctx(tmp_path)
    with_manifest(ctx, models=25)   # the last night built 25 of 28
    audit = ctx.settings.state_dir / "dev" / "mnt_dbt" / "target_drift_audit"
    audit.mkdir(parents=True)
    (audit / "manifest.json").write_text(json.dumps({"nodes": {f"model.p.m{i}": {"resource_type": "model"} for i in range(28)}}))
    assert gitlab.tables_in_project(ctx.settings) == 28


def rehearsal_api(status="success", trace=REHEARSAL_LOG):
    api = FakeApi()
    api.job_list[1] = [{"id": 11, "name": "verify_feature_to_dev", "status": status}]
    api.req = lambda method, path, **kw: type("R", (), {"status_code": 200, "text": trace})()
    return api


def test_rehearsed_counts_of_a_finished_rehearsal_job(tmp_path):
    ctx = make_ctx(tmp_path)
    with_manifest(ctx)
    gitlab._TRACES.clear()
    api = rehearsal_api()
    found = gitlab.rehearsed(ctx.settings, api, api.job_list[1])
    assert found == {"tables": 4, "checks": 3, "tables_total": 28, "seconds": 1.9, "names": ["Customers table"]}
    assert gitlab.rehearsed_words(found) == "4 of 28 tables and 3 data checks in 2 seconds"
    assert gitlab.rehearsed_words({"tables": 1, "checks": 1, "tables_total": 28}) == "1 of 28 tables and 1 data check"
    assert gitlab.rehearsed(ctx.settings, api, [{"id": 11, "name": "verify_feature_to_dev", "status": "running"}]) is None
    assert gitlab.rehearsed(ctx.settings, api, [{"id": 12, "name": "deploy_dev", "status": "success"}]) is None


def test_rehearsed_is_none_without_a_manifest_or_a_readable_log(tmp_path):
    ctx = make_ctx(tmp_path)
    gitlab._TRACES.clear()
    api = rehearsal_api()
    assert gitlab.rehearsed(ctx.settings, api, api.job_list[1]) is None   # no manifest: no total
    with_manifest(ctx)
    gitlab._TRACES.clear()
    api = rehearsal_api(trace="nothing here")
    assert gitlab.rehearsed(ctx.settings, api, api.job_list[1]) is None


def test_wait_pipeline_reports_what_was_rehearsed_when_asked(tmp_path, monkeypatch):
    ctx = make_ctx(tmp_path)
    with_manifest(ctx)
    gitlab._TRACES.clear()
    api = rehearsal_api()
    monkeypatch.setattr(gitlab, "GitLabApi", lambda s: api)
    monkeypatch.setattr(gitlab, "snapshot", lambda s, a: {"tags": []})
    api.pipelines[("d0", "dev")] = {"id": 1, "status": "success", "ref": "dev"}
    plain = gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "ref:dev"}))
    assert plain.summary == "The automatic checks passed."   # other scenarios keep their sentence
    assert plain.data["rehearsed"]["tables"] == 4 and plain.data["rehearsed"]["seconds"] == 1.9
    said = gitlab.wait_pipeline(ctx, gitlab.WaitPipelineParams.model_validate({"for": "ref:dev", "report_rehearsed": True}))
    assert said.summary == "The automatic checks passed. It rehearsed 4 of 28 tables and 3 data checks in 2 seconds."
    assert "Processed: 1 hook | 4 models | 3 tests" in said.details


def test_a_manual_job_records_who_approved_and_when():
    job = {"user": {"name": "Ada Lovelace"}, "started_at": "2026-11-15T17:54:11.367Z"}
    assert gitlab.approval_lines(job) == ["Approved by Ada Lovelace", "Approved on 15 November, 18:54"]   # local time, no "(UTC)"
    assert gitlab.approval_lines({"started_at": "nonsense"}) == []
    assert gitlab.approval_lines({}) == []
