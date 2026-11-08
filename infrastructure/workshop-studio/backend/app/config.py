"""Runtime settings. Everything comes from the environment so the same code runs in the container and on a laptop."""
from __future__ import annotations

import os
from dataclasses import dataclass, field
from pathlib import Path

APP_DIR = Path(__file__).resolve().parent
BACKEND_DIR = APP_DIR.parent
STUDIO_DIR = BACKEND_DIR.parent


def _first_existing(*candidates: str) -> Path:
    for c in candidates:
        if c and Path(c).exists():
            return Path(c)
    return Path(candidates[0])


@dataclass
class Settings:
    # SingleStore (root is needed: fault injection updates _source_channel_config and calls the procedures)
    s2_host: str = field(default_factory=lambda: os.environ.get("S2_HOST", "127.0.0.1"))
    s2_port: int = field(default_factory=lambda: int(os.environ.get("S2_PORT", "3306")))
    s2_user: str = field(default_factory=lambda: os.environ.get("S2_USER", "root"))
    s2_password: str = field(default_factory=lambda: os.environ.get("S2_ROOT_PWD", "WorkshopRootPwd123!"))
    # GitLab
    gitlab_url: str = field(default_factory=lambda: os.environ.get("GITLAB_URL", "http://gitlab.local").rstrip("/"))
    gitlab_pat: str = field(default_factory=lambda: os.environ.get("GITLAB_PAT", "glpat-workshop-root-token-2026"))
    gitlab_project: str = field(default_factory=lambda: os.environ.get("GITLAB_PROJECT", "workshop/dbt-vdp"))
    # Airflow containers are driven through the docker CLI (docker exec airflow-<env> airflow ...)
    airflow_container_prefix: str = field(default_factory=lambda: os.environ.get("AIRFLOW_CONTAINER_PREFIX", "workshop-airflow-"))
    # Airflow metadata DB (MySQL, one database per env: airflow_dev / airflow_prod). Reading dag_run there is a cheap
    # query; the CLI (`airflow dags list-runs`) starts a whole Airflow process and is only the fallback.
    airflow_db_host: str = field(default_factory=lambda: os.environ.get("AIRFLOW_DB_HOST", "mysql"))
    airflow_db_port: int = field(default_factory=lambda: int(os.environ.get("AIRFLOW_DB_PORT", "3306")))
    airflow_db_user: str = field(default_factory=lambda: os.environ.get("AIRFLOW_DB_USER", "airflow"))
    airflow_db_password: str = field(default_factory=lambda: os.environ.get("AIRFLOW_DB_PASSWORD", "airflow_db_password"))
    airflow_db_prefix: str = field(default_factory=lambda: os.environ.get("AIRFLOW_DB_PREFIX", "airflow_"))
    # Content
    scenarios_dir: Path = field(default_factory=lambda: _first_existing(
        os.environ.get("SCENARIOS_DIR", ""), "/app/scenarios", str(STUDIO_DIR / "scenarios")))
    patches_dir: Path = field(default_factory=lambda: _first_existing(
        os.environ.get("PATCHES_DIR", ""), "/app/patches", str(STUDIO_DIR / "patches")))
    static_dir: Path = field(default_factory=lambda: _first_existing(
        os.environ.get("STATIC_DIR", ""), "/app/static", str(STUDIO_DIR / "frontend" / "dist")))
    # The deployed LDV/PRD workspaces (workshop_state/<env>/{dags,dbt}): a live Reset redeploys the baseline code there
    workshop_state_dir: Path = field(default_factory=lambda: Path(os.environ.get("WORKSHOP_STATE_DIR", "/workshop_state")))
    # SingleStore milestones (stopped data-volume snapshots): the script reads S2_CONTAINER / S2_DATA_VOLUME /
    # S2_MILESTONE_VOLUME from the environment. Reset restores `baseline` (made by bootstrap_stack.py).
    milestones_script: Path = field(default_factory=lambda: _first_existing(
        os.environ.get("MILESTONES_SCRIPT", ""), "/app/bin/milestones.sh",
        str(STUDIO_DIR.parent / "manager" / "milestones.sh")))
    state_file: Path = field(default_factory=lambda: Path(os.environ.get(
        "STATE_FILE", "/workshop_state/studio/state.json" if Path("/workshop_state").exists()
        else str(STUDIO_DIR / ".studio-state" / "state.json"))))
    # Behaviour
    probe_interval_s: float = field(default_factory=lambda: float(os.environ.get("PROBE_INTERVAL_S", "5")))
    # dry_run renders every command but executes nothing (also what the offline tests use)
    dry_run: bool = field(default_factory=lambda: os.environ.get("STUDIO_DRY_RUN", "0") == "1")
    host: str = field(default_factory=lambda: os.environ.get("STUDIO_HOST", "127.0.0.1"))
    port: int = field(default_factory=lambda: int(os.environ.get("STUDIO_PORT", "8085")))
    # Host names the browser may use to reach the studio. Anything else is refused (DNS-rebinding protection), and
    # state-changing requests must come from one of them too (see main.guard).
    allowed_hosts: str = field(default_factory=lambda: os.environ.get("STUDIO_ALLOWED_HOSTS", "localhost,127.0.0.1,[::1]"))

    def allowed_host_set(self) -> set[str]:
        return {h.strip().lower() for h in self.allowed_hosts.split(",") if h.strip()}

    def mask(self, text: str) -> str:
        """Never show the PAT / DB passwords in the UI log."""
        for secret in (self.gitlab_pat, self.s2_password, self.airflow_db_password):
            if secret and len(secret) > 6:
                text = text.replace(secret, "***")
        return text
