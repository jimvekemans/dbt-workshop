"""The HTTP API (see spec section 8), the Server-Sent Events stream and the static frontend."""
from __future__ import annotations

import asyncio
import json
import logging
from contextlib import asynccontextmanager
from pathlib import Path
from typing import Any, AsyncIterator, Callable
from urllib.parse import urlparse

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import FileResponse, JSONResponse, StreamingResponse

from .catalog import Catalog, CatalogError, load_catalog, load_registry
from .actions.clients import sweep_throwaway
from .config import Settings
from .engine import BadRequest, Conflict, Engine
from .models import FeederBody, ModeBody, RunBody
from .recorder import Recorder
from .templating import TemplateContext

log = logging.getLogger("studio.main")
SAFE_METHODS = {"GET", "HEAD", "OPTIONS"}
CLIENT_HEADER = "x-studio"   # the UI sends `X-Studio: 1`; a custom header forces a CORS preflight, which is never granted
HEARTBEAT_SECONDS = 15


def _hostname(value: str) -> str:
    """'localhost:8085' -> 'localhost', '[::1]:8085' -> '[::1]'."""
    value = value.strip().lower()
    if value.startswith("["):
        return value[: value.find("]") + 1]
    return value.split(":", 1)[0]


def request_problem(method: str, headers: Any, allowed: set[str]) -> str | None:
    """Why a request must be refused, or None. Protects a powerful local tool (Docker socket, GitLab root token)
    from other web pages open in the same browser:
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


async def sse_stream(engine: Engine, heartbeat: float = HEARTBEAT_SECONDS) -> AsyncIterator[str]:
    """The current state at once, then one `state` event per change (at most 4 per second) and a comment line
    every `heartbeat` seconds so proxies keep the connection open."""
    subscription = engine.broker.subscribe()
    while True:
        outcome = await asyncio.to_thread(subscription.wait, heartbeat)
        if outcome == "closed":
            return
        yield f"event: state\ndata: {json.dumps(engine.state())}\n\n" if outcome == "change" else ": heartbeat\n\n"


def _static_file(root: Path, path: str) -> Path:
    """The requested static file, or index.html (single-page app). Never a path outside `root`."""
    candidate = (root / path).resolve()
    return candidate if candidate.is_file() and root.resolve() in candidate.parents else root / "index.html"


def create_app(settings: Settings | None = None, *, registry: bool = True,
               feeder_factory: Callable[[], Any] | None = None) -> FastAPI:
    """`registry=False` skips importing the real actions (tests register their own). `feeder_factory` builds the
    feeder lazily, so replay mode never needs the stack."""
    settings = settings or Settings.from_env()
    holder: dict[str, Any] = {"engine": None, "catalog": None, "problems": None}
    recorder = Recorder(settings.recordings_dir)

    def make_feeder() -> Any:
        from .actions.clients import s2_connect
        from .feeder import Feeder
        # root connection to `<env>_landing`; generous timeouts because the feeder writes thousands of rows at once
        return Feeder(lambda env: s2_connect(settings, "root", f"{env}_landing", timeout=120),
                      str(settings.data_dir / "feeder.json"))

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        if registry:
            load_registry()
            if stale := sweep_throwaway():
                log.info("removed %d leftover throwaway container(s): %s", len(stale), ", ".join(stale))
        try:
            holder["catalog"] = load_catalog(settings)
        except CatalogError as e:
            holder["problems"] = e.problems
            log.error("%s", e)
        else:
            holder["engine"] = Engine(settings, holder["catalog"], recorder, feeder_factory or make_feeder)
            app.state.engine = holder["engine"]
            holder["engine"].start()
        yield
        if holder["engine"]:
            holder["engine"].stop()

    app = FastAPI(title="Workshop Studio", lifespan=lifespan)
    allowed = settings.allowed_host_set()

    @app.middleware("http")
    async def guard(request: Request, call_next):
        problem = request_problem(request.method, request.headers, allowed)
        if problem:
            return JSONResponse({"detail": problem}, status_code=403)
        return await call_next(request)

    def engine() -> Engine:
        if holder["problems"] is not None:
            raise HTTPException(500, detail={"message": "The scenario files have problems.",
                                             "problems": holder["problems"]})
        return holder["engine"]

    def accepted(operation: Callable[..., None], *args: Any) -> JSONResponse:
        try:
            operation(*args)
        except Conflict as e:
            raise HTTPException(409, str(e)) from e
        except BadRequest as e:
            raise HTTPException(400, str(e)) from e
        return JSONResponse(engine().state(), status_code=202)

    @app.get("/api/health")
    def health() -> dict[str, bool]:
        return {"ok": True}

    @app.get("/api/catalog")
    def get_catalog() -> dict[str, Any]:
        eng = engine()
        catalog: Catalog = eng.catalog
        recorded = lambda sc: {s.id for s in sc.demo.steps} <= recorder.current_steps(  # noqa: E731
            sc.id, {s.id: [i["type"] for i in s.show] for s in sc.demo.steps})
        return {"groups": catalog.groups_payload(recorded), "glossary": catalog.glossary,
                "overview_url": settings.overview_url}

    @app.get("/api/scenarios/{scenario_id}")
    def get_scenario(scenario_id: str) -> dict[str, Any]:
        eng = engine()
        if scenario_id not in eng.catalog.scenarios:
            raise HTTPException(404, "There is no such scenario.")
        sc = eng.catalog.scenarios[scenario_id]
        return eng.catalog.scenario_payload(
            scenario_id, TemplateContext(sc.environment, {}, eng.today_in(sc.environment)),
            usual_seconds=lambda step_id: recorder.usual_seconds(scenario_id, step_id),
            prepare_usual_seconds=recorder.prepare_usual_seconds(scenario_id))

    @app.get("/api/state")
    def get_state() -> dict[str, Any]:
        return engine().state()

    @app.get("/api/events")
    def events() -> StreamingResponse:
        return StreamingResponse(sse_stream(engine()), media_type="text/event-stream",
                                 headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"})

    @app.post("/api/scenarios/{scenario_id}/prepare")
    async def prepare(scenario_id: str) -> JSONResponse:
        return accepted(engine().prepare, scenario_id)

    @app.post("/api/scenarios/{scenario_id}/steps/{step_id}/run")
    async def run_step(scenario_id: str, step_id: str, body: RunBody) -> JSONResponse:
        return accepted(engine().run_step, scenario_id, step_id, body.inputs)

    @app.post("/api/cleanup")
    async def cleanup() -> JSONResponse:
        return accepted(engine().cleanup)

    @app.post("/api/cancel")
    async def cancel() -> JSONResponse:
        return accepted(engine().cancel)

    @app.post("/api/reset")
    async def reset() -> JSONResponse:
        return accepted(engine().reset_state)

    @app.post("/api/mode")
    async def mode(body: ModeBody) -> JSONResponse:
        return accepted(engine().set_mode, body.mode)

    @app.post("/api/feeder")
    async def feeder(body: FeederBody) -> JSONResponse:
        return accepted(engine().set_feeder, body.running)

    static = settings.static_dir
    if static.is_dir():
        @app.get("/{path:path}", include_in_schema=False)
        def frontend(path: str) -> FileResponse:
            if path.startswith("api/"):
                raise HTTPException(404, "Not found.")
            return FileResponse(_static_file(static, path))

    return app


app = create_app()
