import pytest
from fastapi.testclient import TestClient

from app.main import create_app


@pytest.fixture()
def client(settings):
    app = create_app(settings, start_probes=False)
    with TestClient(app, headers={"X-Studio": "1"}) as c:   # the header the UI sends (see main.request_problem)
        yield c


def test_world_endpoint_has_everything_the_frontend_needs(client):
    w = client.get("/api/world").json()
    assert {"groups", "nodes", "edges", "sprint", "scenarios", "state", "log"} <= set(w)
    assert len(w["scenarios"]) == 21
    kafka = next(n for n in w["nodes"] if n["id"] == "channel.kafka")
    assert {k["key"] for k in kafka["knobs"]} >= {"delay_batches", "duplicate_pct", "partial_pct"}


def test_inject_and_step_flow_over_rest(client):
    s = client.post("/api/scenarios/S1_kafka_late/inject").json()
    assert s["phase"] == "resolving" and s["nodes"]["exposure.finance_revenue_report"]["health"] == "fail"
    detail = client.get("/api/scenarios/S1_kafka_late").json()
    assert detail["steps"][2]["hasLive"] and any("delay_batches" in r for r in detail["steps"][2]["rendered"])
    assert client.post("/api/step/2/run").status_code == 400            # out of order
    for _ in range(len(detail["steps"])):
        s = client.post("/api/step/next").json()
    assert s["phase"] == "resolved" and s["counts"]["fail"] == 0
    assert client.post("/api/step/next").status_code == 400
    assert client.post("/api/scenarios/NOPE/inject").status_code == 404


def test_tick_knob_mode_reset(client):
    assert client.post("/api/tick").json()["sprintDay"] == 1
    assert client.post("/api/knob", json={"node": "channel.drive", "key": "duplicate_pct", "value": 50}).json()["nodes"]["channel.drive"]["health"] == "warn"
    assert client.post("/api/knob", json={"node": "channel.drive", "key": "nope", "value": 1}).status_code == 400
    assert client.post("/api/mode", json={"mode": "bogus"}).status_code == 400
    assert client.post("/api/mode", json={"mode": "live"}).json()["mode"] == "live"
    assert client.post("/api/probe").status_code == 400          # dry-run: probes would hit the real stack
    s = client.post("/api/reset").json()
    assert s["sprintDay"] == 0 and s["scenarioId"] is None


def test_websocket_pushes_state_and_logs(client):
    with client.websocket_connect("/ws") as ws:
        assert ws.receive_json()["type"] == "state"
        client.post("/api/tick")
        types, day = set(), 0
        while day != 1:                      # tick publishes: busy state, log line(s), final state
            ev = ws.receive_json()
            types.add(ev["type"])
            if ev["type"] == "state":
                day = ev["state"]["sprintDay"]
        assert types == {"state", "log"}
