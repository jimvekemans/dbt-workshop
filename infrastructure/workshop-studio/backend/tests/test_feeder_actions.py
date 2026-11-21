import sys
from datetime import timedelta
from pathlib import Path
from types import SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

pytest.importorskip("pydantic")
from studio.actions import base, feed  # noqa: E402
from studio.feeder import Feeder  # noqa: E402
from test_feeder_feeder import NOW, FakeDb  # noqa: E402


@pytest.fixture()
def ctx(tmp_path):
    db = FakeDb()
    feeder = Feeder(db.connect, str(tmp_path / "f.json"), seed=5, now=lambda: NOW)
    return base.Ctx(env="dev", settings=SimpleNamespace(), feeder=feeder)


def test_registered():
    assert "feed.condition" in base.ACTIONS and "feed.deliver" in base.ACTIONS
    assert "feed" in base.CLEANERS and "landing" in base.CLEANERS


def test_condition_and_cleaner(ctx):
    spec = base.ACTIONS["feed.condition"]
    params = spec.params(feed="carrier", set={"format_from": "2026-06-10"})
    result = spec.run(ctx, params)
    assert result.touched == {"feed:dev:carrier"} and "10 juni" in result.summary
    assert ctx.feeder.status()["conditions"]["dev"]["carrier"] == {"format_from": "2026-06-10"}
    base.CLEANERS["feed"](ctx, "feed:dev:carrier")
    base.CLEANERS["feed"](ctx, "feed:dev:carrier")          # idempotent
    assert ctx.feeder.status()["conditions"]["dev"] == {}


def test_validation_at_load_time():
    spec = base.ACTIONS["feed.condition"]
    for bad in [{"feed": "orders", "set": {"bots": 1}}, {"feed": "web", "set": {}}, {"feed": "x", "set": {"bots": 1}},
                {"feed": "web", "set": {"bots": 1}, "extra": 1}]:
        with pytest.raises(Exception):
            spec.params(**bad)
    spec.params(feed="carrier", set={"format_from": "{{ day(-4) }}"})   # templated: checked when it runs


def test_deliver_and_landing_cleaner(ctx):
    ctx.feeder.seed("dev", days=5)
    ctx.feeder.set_condition("dev", "carrier", paused=True)
    spec = base.ACTIONS["feed.deliver"]
    result = spec.run(ctx, spec.params(days=3, feeds=["carrier"]))
    assert result.touched == {"landing:dev"}
    assert result.summary.startswith("Pakketscans van de laatste 3 dagen opnieuw verstuurd naar development met de huidige instellingen (0 rijen")
    assert ctx.feeder.today("dev") == NOW.date()
    newest = ctx.feeder.status()["newest"]["dev"]["carrier"]
    assert newest < (NOW - timedelta(days=2)).replace(hour=0, minute=0, second=0).isoformat()
    base.CLEANERS["landing"](ctx, "landing:dev")
    assert ctx.feeder.status()["newest"]["dev"]["carrier"] > newest
