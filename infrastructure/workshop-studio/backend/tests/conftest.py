import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from app.config import Settings  # noqa: E402
from app.engine import Engine  # noqa: E402
from app.loader import load_scenarios, load_world  # noqa: E402


@pytest.fixture()
def settings(tmp_path):
    s = Settings()
    s.state_file = tmp_path / "state.json"
    s.dry_run = True
    s.allowed_hosts = "localhost,127.0.0.1,[::1],testserver"   # TestClient sends Host: testserver
    return s


@pytest.fixture()
def engine(settings):
    world = load_world(settings.scenarios_dir)
    return Engine(settings, world, load_scenarios(settings.scenarios_dir, world))
