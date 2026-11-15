"""Settings read from the environment. Secrets have no defaults: without them live mode is unavailable, replay mode
keeps working."""
from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping

MODES = ("live", "replay")


@dataclass(frozen=True)
class Settings:
    scenarios_dir: Path = Path("/app/scenarios")
    patches_dir: Path = Path("/app/patches")
    recordings_dir: Path = Path("/app/recordings")
    data_dir: Path = Path("/data")                      # state.json: what demos touched and not yet cleaned up
    state_dir: Path = Path("/state")                    # read-only dbt state volumes, one folder per environment
    sql_dir: Path = Path("/workshop/singlestore")       # the SQL files that hold the demo users' passwords
    dbt_project_dir: Path = Path("/workshop/dbt-singlestore-workshop")   # code blocks are read from here
    dev_image_dir: Path = Path("/workshop/dev-image")    # the developer image's recipe (`file: dev-image/Dockerfile`)
    static_dir: Path = Path("/app/static")
    s2_host: str = "singlestore"
    s2_port: int = 3306
    s2_root_pwd: str = ""
    mysql_host: str = "mysql"
    gitlab_url: str = "http://gitlab.local"
    gitlab_pat: str = ""
    mailpit_url: str = "http://mailpit:8025"
    docs_host: str = "workshop-dbt-docs"                 # the container that serves the three documentation sites
    airflow_container_prefix: str = "workshop-airflow-"
    docker_network: str = "workshop_net"
    baseline_tag: str = "v1.0.0"
    feed_seconds: float = 60.0
    mode: str = "live"
    overview_url: str = "http://localhost:8099"
    allowed_hosts: str = "localhost,127.0.0.1,[::1]"

    @classmethod
    def from_env(cls, env: Mapping[str, str] | None = None) -> "Settings":
        """Build settings from environment variables; anything unset keeps its default."""
        env = os.environ if env is None else env
        defaults = cls()

        def text(name: str, default: str) -> str:
            return env.get(name, default)

        def path(name: str, default: Path) -> Path:
            return Path(env.get(name, str(default)))

        mode = text("STUDIO_MODE", defaults.mode)
        if mode not in MODES:
            raise ValueError(f"STUDIO_MODE must be one of {', '.join(MODES)}, not '{mode}'")
        return cls(
            scenarios_dir=path("SCENARIOS_DIR", defaults.scenarios_dir),
            patches_dir=path("PATCHES_DIR", defaults.patches_dir),
            recordings_dir=path("RECORDINGS_DIR", defaults.recordings_dir),
            data_dir=path("DATA_DIR", defaults.data_dir),
            state_dir=path("STATE_DIR", defaults.state_dir),
            sql_dir=path("SQL_DIR", defaults.sql_dir),
            dbt_project_dir=path("DBT_PROJECT_DIR", defaults.dbt_project_dir),
            dev_image_dir=path("DEV_IMAGE_DIR", defaults.dev_image_dir),
            static_dir=path("STUDIO_STATIC_DIR", defaults.static_dir),
            s2_host=text("S2_HOST", defaults.s2_host),
            s2_port=int(text("S2_PORT", str(defaults.s2_port))),
            s2_root_pwd=text("S2_ROOT_PWD", ""),
            mysql_host=text("MYSQL_HOST", defaults.mysql_host),
            gitlab_url=text("GITLAB_URL", defaults.gitlab_url).rstrip("/"),
            gitlab_pat=text("GITLAB_PAT", ""),
            mailpit_url=text("MAILPIT_URL", defaults.mailpit_url).rstrip("/"),
            docs_host=text("DOCS_HOST", defaults.docs_host),
            airflow_container_prefix=text("AIRFLOW_CONTAINER_PREFIX", defaults.airflow_container_prefix),
            docker_network=text("DOCKER_NETWORK", defaults.docker_network),
            baseline_tag=text("STUDIO_BASELINE_TAG", defaults.baseline_tag),
            feed_seconds=float(text("STUDIO_FEED_SECONDS", str(defaults.feed_seconds))),
            mode=mode,
            overview_url=text("OVERVIEW_URL", defaults.overview_url),
            allowed_hosts=text("STUDIO_ALLOWED_HOSTS", defaults.allowed_hosts),
        )

    def allowed_host_set(self) -> set[str]:
        return {h.strip().lower() for h in self.allowed_hosts.split(",") if h.strip()}

    def live_problem(self) -> str | None:
        """A plain sentence on why live mode cannot work, or None when everything it needs is configured."""
        missing = [name for name, value in (("the database password", self.s2_root_pwd),
                                            ("the GitLab access token", self.gitlab_pat)) if not value]
        if not missing:
            return None
        return f"Live mode needs {' and '.join(missing)}, which {'is' if len(missing) == 1 else 'are'} not set. " \
               "Replay mode works without them."
