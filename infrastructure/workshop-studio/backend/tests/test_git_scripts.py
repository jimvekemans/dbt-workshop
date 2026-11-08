"""The E1 git choreography, replayed on a local bare repo (no GitLab needed):
B then A squash-merged to dev -> release branch -> `git revert v1.1.0` really conflicts -> resolution -> promote."""
import shutil
import subprocess
from pathlib import Path

import pytest

from app.actions.base import Ctx
from app.actions.gitscripts import GIT_ENV, promote_release, revert_on_release
from app.patches import apply_to_tree, load_patch

ROOT = Path(__file__).resolve().parents[4]   # repo root: infrastructure/workshop-studio/backend/tests/..
FCT = "models/im_dimensional/fct_olist_orders.sql"


def sh(cwd, *argv):
    import os
    r = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, env={**os.environ, **GIT_ENV})
    assert r.returncode == 0, f"{argv}: {r.stderr}"
    return r.stdout.strip()


@pytest.fixture(params=["linear", "merge-commit"])
def remote(request, tmp_path, settings):
    """bare 'GitLab' repo with main/dev at the dbt baseline (v1.0.0) + a working clone to prepare history."""
    work = tmp_path / "work"
    shutil.copytree(ROOT / "dbt-singlestore-workshop", work,
                    ignore=shutil.ignore_patterns(".git", "target", "dbt_packages", "logs", "airflow"))
    bare = tmp_path / "remote.git"
    sh(tmp_path, "git", "init", "--bare", "-b", "main", str(bare))
    sh(bare, "git", "config", "receive.advertisePushOptions", "true")   # GitLab accepts push options (ci.skip)
    sh(work, "git", "init", "-b", "main")
    sh(work, "git", "add", "-A"); sh(work, "git", "commit", "-m", "baseline")
    sh(work, "git", "tag", "-a", "v1.0.0", "-m", "baseline")   # bootstrap uses annotated tags
    sh(work, "git", "tag", "-a", "2026_DROP1.0", "-m", "baseline")
    sh(work, "git", "remote", "add", "origin", str(bare))
    sh(work, "git", "push", "origin", "main", "--tags")
    sh(work, "git", "checkout", "-b", "dev"); sh(work, "git", "push", "origin", "dev")

    def squash(patch_name, tag):
        patch = load_patch(settings.patches_dir, patch_name)
        sh(work, "git", "checkout", "-b", patch.branch, "dev")
        apply_to_tree(patch, work)
        sh(work, "git", "add", "-A"); sh(work, "git", "commit", "-m", patch.message)
        sh(work, "git", "checkout", "dev")
        if request.param == "linear":
            sh(work, "git", "merge", "--squash", patch.branch); sh(work, "git", "commit", "-m", patch.message)
        else:   # what GitLab does with squash + merge-commit method: squash commit on a side branch, then a merge commit
            sh(work, "git", "checkout", "-b", "squashed-" + tag, "dev")
            sh(work, "git", "merge", "--squash", patch.branch); sh(work, "git", "commit", "-m", patch.message)
            sh(work, "git", "checkout", "dev")
            sh(work, "git", "merge", "--no-ff", "squashed-" + tag, "-m", f"Merge branch '{patch.branch}' into 'dev'")
        sh(work, "git", "tag", "-a", tag, "-m", tag)
        sh(work, "git", "push", "origin", "dev", "--tags")

    squash("feature-domain-b-reviews", "v1.1.0")
    squash("feature-domain-a-payments", "v1.2.0")   # fct has both hkeys on adjacent lines
    sh(work, "git", "push", "origin", "dev:refs/heads/release/2026-Q1")
    return str(bare), work


def test_revert_conflicts_then_resolves_and_promotes(remote, settings):
    url, work = remote
    lines: list[str] = []
    ctx = Ctx(settings=settings, log=lines.append)

    # 1) a plain revert of B really does conflict in fct_olist_orders.sql (the point of the exercise)
    is_merge = len(sh(work, "git", "rev-list", "--parents", "-n", "1", "v1.1.0").split()) > 2
    probe = subprocess.run(["git", "revert", "--no-edit", *(["-m", "1"] if is_merge else []), "v1.1.0"],
                           cwd=work, capture_output=True, text=True)
    assert probe.returncode != 0 and FCT in sh(work, "git", "diff", "--name-only", "--diff-filter=U")
    sh(work, "git", "revert", "--abort")

    # 2) the script resolves it with the Domain-A-only version
    revert_on_release(ctx, branch="release/2026-Q1", tag="v1.1.0", resolution="resolve-release-domain-a-only", url=url)
    assert any("conflict in" in l and "fct_olist_orders.sql" in l for l in lines)
    sh(work, "git", "fetch", "origin")
    rel = "origin/release/2026-Q1"
    files = sh(work, "git", "ls-tree", "-r", "--name-only", rel)
    assert "models/edw_dv_bdv/stg_olist_payments.sql" in files          # Domain A stays
    assert "models/edw_dv_bdv/stg_olist_reviews.sql" not in files       # Domain B is gone
    assert "tests/assert_domain_b_reviews_delivered.sql" not in files
    fct = sh(work, "git", "show", f"{rel}:{FCT}")
    assert "domain_a_hkey" in fct and "domain_b_hkey" not in fct and "<<<<<<<" not in fct
    assert sh(work, "git", "log", "-1", "--format=%s", rel).startswith('Revert "')

    # 3) promote: merge-base reset + --no-ff; main = baseline + the whole release as ONE merge commit
    promote_release(ctx, branch="release/2026-Q1", url=url)
    sh(work, "git", "fetch", "origin")
    assert sh(work, "git", "log", "-1", "--format=%P", "origin/main").count(" ") == 1          # 2 parents = merge commit
    assert sh(work, "git", "log", "-1", "--format=%s", "origin/main") == "Merge branch 'release/2026-Q1' into 'main'"
    assert sh(work, "git", "diff", "--stat", "origin/main", rel) == ""                          # main == release content


def test_q2_release_rejoins_domain_b_after_q1_revert(remote, settings):
    """Because promotion resets main to the merge-base, the feature reverted in Q1 rides along in Q2."""
    url, work = remote
    ctx = Ctx(settings=settings, log=lambda _l: None)
    revert_on_release(ctx, branch="release/2026-Q1", tag="v1.1.0", resolution="resolve-release-domain-a-only", url=url)
    promote_release(ctx, branch="release/2026-Q1", url=url)
    sh(work, "git", "push", "origin", "dev:refs/heads/release/2026-Q2")
    promote_release(ctx, branch="release/2026-Q2", url=url)
    sh(work, "git", "fetch", "origin")
    files = sh(work, "git", "ls-tree", "-r", "--name-only", "origin/main")
    assert "models/edw_dv_bdv/stg_olist_payments.sql" in files and "models/edw_dv_bdv/stg_olist_reviews.sql" in files
    fct = sh(work, "git", "show", f"origin/main:{FCT}")
    assert "domain_a_hkey" in fct and "domain_b_hkey" in fct


def test_reset_branches_peels_annotated_tags(remote, settings):
    from app.actions.gitscripts import reset_branches

    class FakeGL:   # protected-branch + branch/tag cleanup go through the GitLab API; here only the git push matters
        calls: list = []
        def req(self, method, path, **kw):
            self.calls.append((method, path))
            class R:
                def json(self_inner): return []
            return R()
        def p(self, path): return path
        def delete_branch(self, name): pass

    url, work = remote
    reset_branches(Ctx(settings=settings, log=lambda _l: None), url=url, gl=FakeGL())
    sh(work, "git", "fetch", "origin")
    assert sh(work, "git", "rev-parse", "origin/dev") == sh(work, "git", "rev-parse", "v1.0.0^{commit}")
    assert ("POST", "/protected_branches") in FakeGL.calls          # re-protected


def test_reset_push_skips_ci(remote, settings):
    """Regression: the force-push of dev to the baseline must carry `-o ci.skip` or tag_dev tags the baseline commit."""
    from app.actions.gitscripts import reset_branches
    seen: list[list[str]] = []

    class SpyCtx(Ctx):
        def sh(self, argv, **kw):
            seen.append(list(argv))
            return super().sh(argv, **kw)

    class FakeGL:
        def req(self, method, path, **kw):
            class R:
                def json(self_inner): return []
            return R()
        def p(self, path): return path
        def delete_branch(self, name): pass

    url, _ = remote
    reset_branches(SpyCtx(settings=settings, log=lambda _l: None), url=url, gl=FakeGL())
    pushes = [a for a in seen if a[:2] == ["git", "push"] and "--force" in a]
    assert pushes and all("ci.skip" in a for a in pushes)
