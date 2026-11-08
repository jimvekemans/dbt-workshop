"""FastAPI app: REST + WebSocket for the Workshop Studio. Binds to 127.0.0.1 only (see docker-compose)."""
from __future__ import annotations

import asyncio
import threading
import time
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any

from urllib.parse import urlparse

from fastapi import FastAPI, HTTPException, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles
from pydantic import BaseModel

from .actions.base import Cancelled, PreconditionFailed
from .config import Settings
from .engine import Busy, Engine
from .loader import ContentError, load_scenarios, load_world
from .probes import run_probes

SAFE_METHODS = {"GET", "HEAD", "OPTIONS"}
CLIENT_HEADER = "x-studio"   # the UI sends `X-Studio: 1`; a custom header forces a CORS preflight, which this app never grants


def _hostname(value: str) -> str:
    """'localhost:8085' -> 'localhost', '[::1]:8085' -> '[::1]'."""
    value = value.strip().lower()
    if value.startswith("["):
        return value[: value.find("]") + 1]
    return value.split(":", 1)[0]


def request_problem(method: str, headers, allowed: set[str]) -> str | None:
    """Why a request must be refused, or None. Protects a powerful local tool (Docker socket, GitLab root PAT) from
    other web pages open in the same browser:
      - Host must be one of the studio's own names (DNS rebinding: evil.example resolving to 127.0.0.1),
      - state-changing requests must carry X-Studio (cross-site pages cannot add it without a CORS preflight)
        and, when the browser sends an Origin, come from an allowed host."""
    if _hostname(headers.get("host", "")) not in allowed:
        return "host not allowed (set STUDIO_ALLOWED_HOSTS to add one)"
    if method in SAFE_METHODS:
        return None
    origin = headers.get("origin")
    if origin and _hostname(urlparse(origin).netloc) not in allowed:
        return f"cross-origin request from {origin} refused"
    if headers.get(CLIENT_HEADER) != "1":
        return "missing X-Studio header (the studio UI sends it; with curl add -H 'X-Studio: 1')"
    return None


class ModeBody(BaseModel):
    mode: str


class DryRunBody(BaseModel):
    dry_run: bool


class KnobBody(BaseModel):
    node: str
    key: str
    value: Any


def build_engine(settings: Settings) -> Engine:
    world = load_world(settings.scenarios_dir)
    return Engine(settings, world, load_scenarios(settings.scenarios_dir, world))


def create_app(settings: Settings | None = None, *, start_probes: bool = True) -> FastAPI:
    settings = settings or Settings()
    engine = build_engine(settings)
    clients: set[asyncio.Queue] = set()
    loop_ref: dict[str, asyncio.AbstractEventLoop] = {}

    def broadcast(event: dict) -> None:           # called from worker threads
        loop = loop_ref.get("loop")
        if loop is None:
            return
        for q in list(clients):
            loop.call_soon_threadsafe(_offer, q, event)

    def _offer(q: asyncio.Queue, event: dict) -> None:
        if q.full():
            if event.get("type") == "state":      # drop stale snapshots, keep logs
                return
            try:
                q.get_nowait()
            except asyncio.QueueEmpty:
                pass
        q.put_nowait(event)

    engine.listeners.append(broadcast)
    stop = threading.Event()

    def probe_once() -> None:
        # probe_since: results older than the last live Reset belong to a previous session (see probes.py)
        observed, status = run_probes(settings, engine.world, engine.cache, since=engine.state.get("probe_since"))
        engine.set_observed(observed, status)

    # --- scenario YAML hot reload: "edit the YAML and refresh the page" (checked on GET /api/world)
    def content_signature() -> tuple:
        out = []
        for p in sorted(settings.scenarios_dir.glob("*.yaml")):
            try:
                st = p.stat()
                out.append((p.name, st.st_mtime_ns, st.st_size))
            except OSError:
                pass
        return tuple(out)

    content = {"sig": content_signature(), "error": None}

    def maybe_reload() -> None:
        sig = content_signature()
        if sig == content["sig"]:
            return
        try:
            world = load_world(settings.scenarios_dir)
            scenarios = load_scenarios(settings.scenarios_dir, world)
        except ContentError as e:   # keep the last good content; the page shows why the edit was not taken
            if content["error"] != str(e):
                engine.log(f"Scenario YAML not reloaded: {e}", "error")
            content["error"] = str(e)
            return
        try:
            engine.reload_content(world, scenarios)
        except Busy:
            return                  # an operation is running: the next refresh picks it up
        content["sig"], content["error"] = sig, None

    engine.probe_now = probe_once

    def probe_loop() -> None:
        while not stop.wait(settings.probe_interval_s):
            if engine.state["mode"] == "live" and not engine.state["dry_run"]:
                try:
                    probe_once()
                except Exception as e:  # noqa: BLE001
                    engine.log(f"probe error: {e}", "warn")

    @asynccontextmanager
    async def lifespan(_app: FastAPI):
        loop_ref["loop"] = asyncio.get_running_loop()
        if start_probes:
            threading.Thread(target=probe_loop, daemon=True, name="probes").start()
        yield
        stop.set()

    app = FastAPI(title="Workshop Studio", lifespan=lifespan)
    app.state.engine = engine
    allowed = settings.allowed_host_set()

    @app.middleware("http")
    async def guard(request: Request, call_next):
        problem = request_problem(request.method, request.headers, allowed)
        if problem:
            return JSONResponse({"detail": problem}, status_code=403)
        return await call_next(request)

    async def op(fn, *args):
        try:
            return await asyncio.to_thread(fn, *args)
        except Busy as e:
            raise HTTPException(409, f"busy: {e}") from e
        except PreconditionFailed as e:            # wrong starting state: nothing ran, the message says what to do
            raise HTTPException(412, str(e)) from e
        except Cancelled as e:
            raise HTTPException(409, f"cancelled: {e}") from e
        except (KeyError, ValueError) as e:
            raise HTTPException(400, str(e)) from e
        except Exception as e:  # noqa: BLE001 - live action failures: show the reason in the UI, state.error has it too
            raise HTTPException(502, f"{type(e).__name__}: {e}") from e

    @app.get("/api/world")
    def world():
        maybe_reload()
        w = engine.world.model_dump(mode="json")
        return {**w, "scenarios": engine.catalogue(), "state": engine.snapshot(), "log": list(engine.log_lines),
                "contentError": content["error"]}

    @app.get("/api/state")
    def state():
        return engine.snapshot()

    @app.get("/api/scenarios/{sid}")
    def scenario(sid: str):
        if sid not in engine.scenarios:
            raise HTTPException(404, f"unknown scenario {sid}")
        return engine.scenario_detail(sid)

    @app.post("/api/mode")
    async def mode(body: ModeBody):
        return await op(engine.set_mode, body.mode)

    @app.post("/api/dry-run")
    async def dry_run(body: DryRunBody):
        return await op(engine.set_dry_run, body.dry_run)

    @app.post("/api/knob")
    async def knob(body: KnobBody):
        return await op(engine.set_knob, body.node, body.key, body.value)

    @app.post("/api/tick")
    async def tick():
        return await op(engine.tick)

    @app.post("/api/reset")
    async def reset():
        return await op(engine.reset)

    @app.get("/api/milestones")
    async def milestones():
        return {"milestones": await asyncio.to_thread(engine.milestones)}

    @app.post("/api/milestones/{name}/{milestone_op}")
    async def milestone(name: str, milestone_op: str):
        return await op(engine.milestone, milestone_op, name)

    @app.post("/api/scenarios/{sid}/inject")
    async def inject(sid: str):
        if sid not in engine.scenarios:
            raise HTTPException(404, f"unknown scenario {sid}")
        return await op(engine.inject, sid)

    @app.post("/api/step/next")
    async def step_next():
        return await op(engine.run_step, None)

    @app.post("/api/step/{n}/run")
    async def step_run(n: int):
        return await op(engine.run_step, n - 1)

    @app.post("/api/scenario/clear")
    async def clear():
        return await op(engine.clear_scenario)

    @app.post("/api/cancel")
    def cancel():   # not through op(): it has to work while another operation holds the engine
        try:
            return engine.cancel()
        except ValueError as e:
            raise HTTPException(400, str(e)) from e

    @app.post("/api/probe")
    async def probe():
        if engine.state["mode"] != "live" or engine.state["dry_run"]:
            raise HTTPException(400, "probes only run in Live mode without dry-run")
        await asyncio.to_thread(probe_once)
        return engine.snapshot()

    @app.websocket("/ws")
    async def ws(sock: WebSocket):
        # the HTTP guard does not see websockets: same Host check, and a browser Origin must be one of ours
        origin = sock.headers.get("origin")
        if request_problem("GET", sock.headers, allowed) or (origin and _hostname(urlparse(origin).netloc) not in allowed):
            await sock.close(code=1008)
            return
        await sock.accept()
        q: asyncio.Queue = asyncio.Queue(maxsize=200)
        clients.add(q)
        try:
            await sock.send_json({"type": "state", "state": engine.snapshot()})
            for entry in list(engine.log_lines)[-60:]:
                await sock.send_json({"type": "log", "entry": entry})
            while True:
                await sock.send_json(await q.get())
        except (WebSocketDisconnect, RuntimeError):
            pass
        finally:
            clients.discard(q)

    static: Path = settings.static_dir
    if static.is_dir() and (static / "index.html").exists():
        if (static / "assets").is_dir():
            app.mount("/assets", StaticFiles(directory=static / "assets"), name="assets")

        @app.get("/{path:path}", include_in_schema=False)
        def spa(path: str):
            target = (static / path).resolve()
            if path and target.is_file() and static.resolve() in target.parents:
                return FileResponse(target)
            return FileResponse(static / "index.html")

    return app


def run() -> None:
    import uvicorn
    s = Settings()
    uvicorn.run(create_app(s), host=s.host, port=s.port, log_level="info")


if __name__ == "__main__":
    run()
