from __future__ import annotations

import asyncio
import json
from dataclasses import replace

import pytest
import yaml
from fastapi.testclient import TestClient

from studio.main import create_app, request_problem, sse_stream
from tests.conftest import FakeFeeder, scenario, write_catalog

pytestmark = pytest.mark.usefixtures("fake_registry")


HEADERS = {"X-Studio": "1"}


@pytest.fixture
def client(settings, stack):
    app = create_app(settings, registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost:8085") as c:
        app.state.engine.wait_idle(10)                                   # the startup baseline check
        yield c


def post(client, path, body=None):
    return client.post(f"/api{path}", json=body or {}, headers=HEADERS)


def idle(client):
    client.app.state.engine.wait_idle(10)
    return client.get("/api/state").json()


def test_guard_rules():
    ok = {"host": "localhost:8085", "x-studio": "1"}
    assert request_problem("POST", ok, {"localhost"}) is None
    assert "host not allowed" in request_problem("GET", {"host": "evil.example"}, {"localhost"})
    assert "X-Studio" in request_problem("POST", {"host": "localhost"}, {"localhost"})
    assert "cross-origin" in request_problem("POST", {**ok, "origin": "http://evil.example"}, {"localhost"})
    assert request_problem("GET", {"host": "[::1]:8085"}, {"[::1]"}) is None


def test_guard_over_http(client):
    assert client.post("/api/cleanup").status_code == 403
    assert client.get("/api/health", headers={"Host": "evil.example"}).status_code == 403
    assert client.get("/api/health").json() == {"ok": True}


def test_catalog_and_scenario(client):
    body = client.get("/api/catalog").json()
    assert [s["id"] for s in body["groups"][0]["scenarios"]] == ["demo-one", "demo-two"]
    assert body["glossary"] == {"robots": "Programs that do chores."} and not body["groups"][0]["scenarios"][0]["recorded"]
    sc = client.get("/api/scenarios/demo-one").json()
    assert sc["code"][0]["lines"][0]["n"] == 2 and sc["demo"]["steps"][1]["inputs"][1]["default"]
    assert client.get("/api/scenarios/ghost").status_code == 404


def test_catalog_error_is_served_as_500_with_the_list(settings, tmp_path, stack):
    folder = write_catalog(tmp_path / "broken", [scenario("demo-one", group="nope")])
    app = create_app(replace(settings, scenarios_dir=folder), registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost") as c:
        for path in ("/api/catalog", "/api/state"):
            response = c.get(path)
            assert response.status_code == 500 and "group 'nope'" in response.json()["detail"]["problems"][0]
        assert c.get("/api/health").status_code == 200


def test_happy_path_prepare_run_cleanup(client, stack):
    assert client.get("/api/state").json()["baseline"]["status"] == "ready"
    response = post(client, "/scenarios/demo-one/prepare")
    assert response.status_code == 202 and response.json()["job"]["kind"] in ("prepare", None)
    assert idle(client)["demo"]["status"] == "ready"
    assert post(client, "/scenarios/demo-one/steps/second/run", {"inputs": {"pick": "b"}}).status_code == 202
    state = idle(client)
    assert state["demo"]["steps"][1]["status"] == "done" and state["leftovers"]
    assert post(client, "/cleanup").status_code == 202
    state = idle(client)
    assert state["leftovers"] == [] and state["demo"] is None
    assert client.get("/api/catalog").json()["groups"][0]["scenarios"][0]["recorded"] is False   # only 1 of 2 steps


def test_bad_requests_and_conflicts(client, stack):
    assert post(client, "/scenarios/ghost/prepare").status_code == 400
    assert post(client, "/scenarios/demo-one/steps/first/run").status_code == 409                 # demo not open
    assert post(client, "/scenarios/demo-one/prepare").status_code == 202
    idle(client)
    assert post(client, "/scenarios/demo-one/steps/second/run", {"inputs": {"pick": "z"}}).status_code == 400
    assert post(client, "/mode", {"mode": "sideways"}).status_code == 422


def test_busy_is_409_with_a_plain_sentence(client, stack):
    client.app.state.engine.catalog.scenarios["demo-one"].demo.setup[0] = {"action": "fake.block"}
    assert post(client, "/scenarios/demo-one/prepare").status_code == 202
    response = post(client, "/cleanup")
    assert response.status_code == 409 and "klaar is" in response.json()["detail"]
    assert post(client, "/cancel").status_code == 202
    idle(client)


def test_mode_and_feeder(client, settings, stack):
    assert post(client, "/feeder", {"running": False}).json()["feeder"]["running"] is False
    assert post(client, "/mode", {"mode": "replay"}).status_code == 202
    assert post(client, "/feeder", {"running": True}).status_code == 409
    assert client.get("/api/state").json()["mode"] == "replay"


def test_live_refused_without_secrets(settings, stack):
    app = create_app(replace(settings, s2_root_pwd="", mode="replay"), registry=False)
    with TestClient(app, base_url="http://localhost") as c:
        response = c.post("/api/mode", json={"mode": "live"}, headers=HEADERS)
        assert response.status_code == 409 and "De livemodus heeft" in response.json()["detail"]
        assert c.get("/api/state").json()["mode"] == "replay"


def test_sse_sends_state_first_then_heartbeats(client):
    async def first_two():
        stream = sse_stream(client.app.state.engine, heartbeat=0.1)
        first, second = await stream.__anext__(), await stream.__anext__()
        await stream.aclose()
        return first, second

    first, second = asyncio.run(first_two())
    assert first.startswith("event: state\ndata: ") and json.loads(first.split("data: ")[1])["mode"] == "live"
    assert second == ": heartbeat\n\n"


def test_static_frontend_with_index_fallback(settings, tmp_path, stack):
    static = tmp_path / "static"
    (static / "assets").mkdir(parents=True)
    (static / "index.html").write_text("<html>app</html>")
    (static / "assets" / "a.js").write_text("js")
    (tmp_path / "secret.txt").write_text("no")
    app = create_app(replace(settings, static_dir=static), registry=False, feeder_factory=lambda: FakeFeeder(stack))
    with TestClient(app, base_url="http://localhost") as c:
        assert c.get("/assets/a.js").text == "js" and c.get("/s/demo-one/code").text == "<html>app</html>"
        assert c.get("/../secret.txt").text == "<html>app</html>"
        assert c.get("/api/nothing").status_code == 404
