"""Live reset: the SingleStore warehouse back to milestone `baseline` (every landing and model table, the run log,
users and grants exactly as bootstrap left them - manager/milestones.sh), GitLab back to the baseline tags, the
baseline code redeployed to the LDV and PRD workspaces, the studio's lever Variables deleted in both Airflows.
Without a `baseline` milestone (stack bootstrapped before milestones existed) the old path runs: channels + landing
re-seeded, model tables keep their last build. `infrastructure/workshop.sh reset` (manager container) rebuilds everything and
takes a fresh `baseline`."""
from __future__ import annotations

import io
import shutil
import tarfile
import tempfile
from pathlib import Path

from .airflow import LEVER_VARIABLES
from .base import Action, ActionError, Ctx, Result, execute, register
from .gitlab import GitLab
from .gitscripts import reset_branches
from .milestones import BASELINE, milestone_exists

# what the CI deploy (.deploy-airflow) puts in /workshop_state/<env> (volumes workshop_dev / workshop_prod)
BASELINE_DEPLOYMENTS = (("dev", "v1.0.0"), ("prod", "2026_DROP1.0"))
# workspace entries that are generated in place, not part of the repository: kept
KEEP_IN_WORKSPACE = {"dbt_packages", "target", "logs", "artifacts", "package-lock.yml"}


def _replace_dir_contents(dst: Path, src: Path, keep: set[str] = frozenset()) -> None:
    dst.mkdir(parents=True, exist_ok=True)
    for entry in dst.iterdir():
        if entry.name in keep:
            continue
        shutil.rmtree(entry) if entry.is_dir() and not entry.is_symlink() else entry.unlink()
    for entry in src.iterdir():
        target = dst / entry.name
        shutil.copytree(entry, target, ignore=shutil.ignore_patterns("__pycache__")) if entry.is_dir() else shutil.copy2(entry, target)


def _make_writable(root: Path) -> None:
    for p in [root, *root.rglob("*")]:
        try:
            p.chmod(p.stat().st_mode | 0o666 | (0o111 if p.is_dir() else 0))
        except OSError:
            pass


def redeploy_baseline(ctx: Ctx) -> list[str]:
    """The deploy job, without CI: the baseline tag's DAGs and dbt project into /workshop_state/<env>. Without it a Reset
    rolls GitLab back while LDV keeps running whatever the last tag_dev published (e.g. a scenario's fix), so the
    next run of that scenario has nothing left to detect."""
    gl = GitLab(ctx)
    state = ctx.settings.workshop_state_dir
    done = []
    for env, tag in BASELINE_DEPLOYMENTS:
        root = state / env
        if not (root / "dbt").is_dir():
            raise ActionError(f"{root}/dbt does not exist: are the workshop_dev / workshop_prod volumes mounted "
                              "(docker-compose.yml, WORKSHOP_STATE_DIR)?")
        archive = gl.req("GET", gl.p("/repository/archive.tar.gz"), params={"sha": tag}).content
        with tempfile.TemporaryDirectory() as tmp:
            with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
                tar.extractall(tmp, filter="data")
            src = next(Path(tmp).iterdir())          # <project>-<tag>-<sha>/
            _replace_dir_contents(root / "dbt", src, KEEP_IN_WORKSPACE)
            dags = root / "dags"
            _replace_dir_contents(dags, src / "airflow" / "dags")
            _replace_dir_contents(dags / "utils", src / "airflow" / "utils")
        _make_writable(root / "dbt")
        _make_writable(root / "dags")
        done.append(f"{tag} -> {env}")
    ctx.say("Redeployed the baseline code: " + ", ".join(done))
    return done


@register
class Reset(Action):
    name = "reset"

    def render(self, p, ctx):
        return ["infrastructure/workshop.sh reset   # full equivalent (also rebuilds every model from scratch)",
                f"manager/milestones.sh restore {p.get('milestone', BASELINE)}   # whole warehouse; "
                "without it: CALL sp_reset_channel_config(); CALL sp_reset_olist_landing(90, ...)",
                "git push --force origin v1.0.0:dev 2026_DROP1.0:main  # + drop other branches/tags",
                "deploy v1.0.0 -> workshop_state/dev, 2026_DROP1.0 -> workshop_state/prod  # DAGs + dbt project",
                "airflow variables delete " + " | ".join(LEVER_VARIABLES) + "   # dev + prod"]

    def run(self, p, ctx):
        name = p.get("milestone", BASELINE)
        restored = bool(name) and milestone_exists(ctx, name)
        if restored:
            # first: SingleStore restarts, so nothing else may be talking to it yet
            execute("milestone", {"op": "restore", "name": name}, ctx)
        else:
            ctx.say(f"No SingleStore milestone '{name}': re-seeding channels and landing only "
                    "(run `infrastructure/workshop.sh reset` once to create it)")
            execute("reset_channels", {"env": "both"}, ctx)
            execute("reset_landing", {"env": "both", "days": int(p.get("days", 90))}, ctx)
        if p.get("gitlab", True):
            reset_branches(ctx)
            # LDV/PRD run what the last deploy put there (tag_dev publishes every merged fix to LDV): back to baseline
            redeploy_baseline(ctx)
        if p.get("airflow", True):   # levers a scenario switched on (exclude list, .dbtignore, env vars)
            for key in LEVER_VARIABLES:
                execute("airflow_variable", {"env": "both", "key": key, "delete": True}, ctx)
        if not restored:
            ctx.say("Note: tables keep their last build; the next daily load rebuilds them from the baseline code. "
                    "`infrastructure/workshop.sh reset` also rebuilds every model from scratch.")
        return Result(output="reset complete" + (f" (warehouse restored to milestone '{name}')" if restored else ""))
