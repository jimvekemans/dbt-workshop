"""The state machine behind the demos: one job at a time, a persisted set of touched resources, cleanup derived from
that set, the baseline every demo starts from, live evidence while a step runs, and replay of recordings.

Everything the engine tells the audience is a plain sentence; tracebacks only go to the "technical details".
"""
from __future__ import annotations

import copy
import json
import logging
import os
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any, Callable

import pymysql
from pydantic import ValidationError

from .actions import base, clients
from .actions.base import Cancelled, Ctx, Result, StepFailed
from .catalog import Catalog
from .config import Settings
from . import names
from .models import ENVIRONMENTS, Scenario, Step
from .recorder import FrameCollector, Recorder, replay
from .templating import TemplateContext, TemplateError, render, render_input, render_item

log = logging.getLogger("studio.engine")

EVIDENCE_EVERY = 2.0          # seconds between evidence reads while a step runs
STACK_EVERY = 10.0
SSE_MIN_GAP = 0.25            # at most 4 state pushes per second
BASELINE_DAYS = 90
EVIDENCE_ERROR = "We could not read this just now."
CHECK_TIMEOUT = 5.0           # seconds an outcome check may take
CHECK_FAILED = "The step finished, but its result could not be checked."
STOPPED = "Stopped before it finished."
FULL_CHECK_EVERY = 600.0      # seconds after which a prepare does the full reality check again
BASELINE_WAIT = 300.0         # seconds a request waits for the running baseline check before it gives up
DATABASE_WAIT = 300.0         # seconds the baseline check waits for the database after a cold start
BASELINE_ATTEMPTS = 4          # tries of the baseline check in one go (the Airflows start after the database)
BASELINE_RETRY_AFTER = 20.0    # seconds between those tries
DATABASE_POLL = 3.0            # seconds between looks at the database while the check waits for it
RESET_CANCEL_WAIT = 60.0       # seconds a reset waits for a running baseline check to stop
WAITING = "Waiting until the practice system is ready."
RUN_CLOCK_SLACK = timedelta(seconds=2)   # the database clock may run a little behind this one

NAMESPACES = {"feed": "the demo data feed", "airflow": "the scheduler", "dbt": "the build tool",
              "sql": "the database", "gitlab": "GitLab", "git": "GitLab", "deploy": "the deployment",
              "mail": "the mail inbox", "precommit": "the code checks"}
ACTION_LABELS = {"feed": "Preparing the demo data", "airflow": "Running the scheduled load", "dbt": "Building the tables",
                 "sql": "Talking to the database", "gitlab": "Working in GitLab", "git": "Working in GitLab",
                 "deploy": "Switching the running version", "mail": "Clearing the inbox",
                 "precommit": "Checking the code", "wait": "Waiting a moment"}
LEFTOVER_LABELS = {"variable": "A setting changed in {env}", "feed": "A delivery rule changed in {env}",
                   "landing": "Demo data delivered to {env}", "models": "Tables rebuilt in {env}",
                   "version": "A different version running in {env}", "developer": "A personal work copy in {env}",
                   "git": "Demo copies of the code in GitLab", "sql": "A database change made for a demo",
                   "ci": "Scratch tables of the safety check", "mail": "Demo e-mails in the inbox"}


class Conflict(Exception):
    """Refused because of the current state (another job runs, wrong mode). The message is a plain sentence."""


class BadRequest(ValueError):
    """The request names something that does not exist or a value that is not allowed."""


def leftover_label(resource: str) -> str:
    """A plain sentence for a touched resource such as `variable:dev:dbt_env_vars`."""
    prefix, _, rest = resource.partition(":")
    env = rest.split(":")[0] if rest.split(":")[0] in ENVIRONMENTS else "the demo environment"
    return LEFTOVER_LABELS.get(prefix, "Something changed by a demo").format(env=names.ENV_WORDS.get(env, env))


def _clean_rank(resource: str) -> int:
    prefix = resource.partition(":")[0]
    return base.CLEAN_ORDER.index(prefix) if prefix in base.CLEAN_ORDER else len(base.CLEAN_ORDER)


# --- persistence and pub/sub -------------------------------------------------------------------------------------

class TouchedStore:
    """The touched set, `resource -> scenario that caused it`, in a JSON file written atomically."""

    def __init__(self, path: Path):
        self.path, self._items = path, {}
        try:
            self._items = dict(json.loads(path.read_text(encoding="utf-8")).get("touched", {}))
        except FileNotFoundError:
            pass
        except (ValueError, OSError):
            log.exception("%s is unreadable: starting with nothing to clean up", path)

    def items(self) -> dict[str, str]:
        return dict(self._items)

    def clear(self) -> None:
        self._items = {}
        self._save()

    def add(self, resources: set[str], scenario_id: str) -> None:
        new = {r for r in resources if r not in self._items}
        if new:
            self._items.update({r: scenario_id for r in new})
            self._save()

    def remove(self, resource: str) -> None:
        if self._items.pop(resource, None) is not None:
            self._save()

    def _save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".json.tmp")
        tmp.write_text(json.dumps({"touched": self._items}, indent=1), encoding="utf-8")
        os.replace(tmp, self.path)


class Broker:
    """Tells subscribers that the state changed. They fetch the newest state themselves, so bursts of changes
    coalesce into at most one push per `SSE_MIN_GAP`."""

    def __init__(self) -> None:
        self._cond, self._version, self._closed = threading.Condition(), 0, False

    def notify(self) -> None:
        with self._cond:
            self._version += 1
            self._cond.notify_all()

    def close(self) -> None:
        with self._cond:
            self._closed = True
            self._cond.notify_all()

    def subscribe(self) -> "Subscription":
        return Subscription(self)


class Subscription:
    def __init__(self, broker: Broker):
        self._broker, self._seen, self._last = broker, -1, 0.0

    def wait(self, timeout: float) -> str:
        """"change" when the state changed since the last call (the first call returns it at once), "timeout"
        when nothing happened, "closed" when the broker is shut down."""
        cond = self._broker._cond
        with cond:
            if not cond.wait_for(lambda: self._broker._version != self._seen or self._broker._closed, timeout):
                return "timeout"
            if self._broker._closed:
                return "closed"
        gap = SSE_MIN_GAP - (time.monotonic() - self._last)
        if gap > 0:
            time.sleep(gap)
        self._last = time.monotonic()
        with cond:
            self._seen = self._broker._version
        return "change"


@dataclass
class Check:
    """One baseline requirement: `ok()` tells whether it holds, `fix()` makes it hold."""

    label: str
    ok: Callable[[], bool]
    fix: Callable[[], None]


# --- the engine --------------------------------------------------------------------------------------------------

class Engine:
    def __init__(self, settings: Settings, catalog: Catalog, recorder: Recorder,
                 feeder_factory: Callable[[], Any]):
        self.settings, self.catalog, self.recorder = settings, catalog, recorder
        self._feeder_factory, self._feeder = feeder_factory, None
        self._feed_paused = False     # the presenter switched the background data off
        self.broker = Broker()
        self._lock = threading.RLock()
        self._store = TouchedStore(settings.data_dir / "state.json")
        self._live_problem = settings.live_problem()
        self.mode = "replay" if self._live_problem else settings.mode
        self._job: dict[str, Any] | None = None
        self._job_actions: list[dict[str, Any]] = []
        self._cancel, self._thread = threading.Event(), None
        self._demo: dict[str, Any] | None = None
        self._chosen: dict[str, Any] = {}
        self._last_result: Result | None = None
        self._results: dict[str, Result] = {}
        self._notice: dict[str, str] | None = None
        self._stack: list[dict[str, Any]] = []
        self._baseline = self._idle_baseline()
        # The full reality check (data, version, tables per environment, slow: it reads the version through Airflow)
        # runs at start-up, after a reset, after a failed or cancelled job, for what a clean-up touched and every
        # FULL_CHECK_EVERY seconds; in between a prepare only does the quick check (one SQL round trip per environment).
        self._full_due = True
        self._last_full: float | None = None
        self._cleaned_envs: set[str] = set()
        self._queued: dict[str, str] | None = None   # a request that waits for the baseline check to finish
        self._stopping = threading.Event()
        self._stack_thread: threading.Thread | None = None

    # -- lifecycle ---------------------------------------------------------------------------------------------

    def start(self) -> None:
        """Start the background threads: the stack strip, and in live mode the feeder and the baseline check."""
        self._stack_thread = threading.Thread(target=self._stack_loop, daemon=True, name="stack-strip")
        self._stack_thread.start()
        if self.mode == "live":
            self._go_live()

    def stop(self) -> None:
        self._stopping.set()
        self._cancel.set()
        self.broker.close()
        if self._feeder is not None:
            self._feeder.stop()
        self.wait_idle(10)

    def wait_idle(self, timeout: float | None = None) -> None:
        """Block until the running job (if any) has finished."""
        thread = self._thread
        if thread is not None:
            thread.join(timeout)

    def _go_live(self) -> None:
        feeder = self._get_feeder()
        feeder.start_background(self.settings.feed_seconds,
                                should_pause=lambda: self._job is not None or self._feed_paused)
        self.ensure_baseline()

    def _get_feeder(self) -> Any:
        if self._feeder is None:
            self._feeder = self._feeder_factory()
        return self._feeder

    def _publish(self) -> None:
        self.broker.notify()

    # -- state payload -----------------------------------------------------------------------------------------

    def state(self) -> dict[str, Any]:
        with self._lock:
            leftovers = sorted({(leftover_label(r), s) for r, s in self._store.items().items()})
            job = copy.deepcopy(self._job)
            if job:
                job["actions"] = copy.deepcopy(self._job_actions)
            return {"mode": self.mode, "live_unavailable": self._live_problem, "stack": copy.deepcopy(self._stack),
                    "baseline": copy.deepcopy(self._baseline), "feeder": self._feeder_status(), "job": job,
                    "leftovers": [{"label": label, "from_scenario": scenario} for label, scenario in leftovers],
                    "notice": copy.deepcopy(self._notice), "demo": copy.deepcopy(self._demo),
                    "waiting": copy.deepcopy(self._queued)}

    def _feeder_status(self) -> dict[str, Any]:
        if self._feeder is None:
            return {"running": False, "simulated_day": None, "last_delivery": None}
        status = self._feeder.status()
        days = status.get("simulated_day") or {}
        return {"running": bool(status.get("running")) and not self._feed_paused,
                "simulated_day": days.get("dev") if isinstance(days, dict) else days,
                "simulated_days": days, "last_delivery": status.get("last_delivery"),
                "rows": status.get("rows", {})}

    def _idle_baseline(self) -> dict[str, Any]:
        if self.mode == "replay":
            return {"status": "ready", "detail": "Replay mode: nothing runs on the stack.", "progress": []}
        return {"status": "missing", "detail": "Not checked yet.", "progress": []}

    # -- jobs --------------------------------------------------------------------------------------------------

    def _start_job(self, kind: str, scenario: str | None, step: str | None, label: str,
                   body: Callable[[threading.Event], None]) -> None:
        with self._lock:
            if self._job:
                raise Conflict(f"Please wait: \"{self._job['label']}\" is still running.")
            self._cancel = threading.Event()
            self._job = {"kind": kind, "scenario": scenario, "step": step, "label": label,
                         "started": time.time(), "cancellable": True}
            self._job_actions, self._notice = [], None
            cancel = self._cancel
            self._thread = threading.Thread(target=self._job_main, args=(body, cancel), daemon=True, name=f"job-{kind}")
        self._publish()
        self._thread.start()

    def _job_main(self, body: Callable[[threading.Event], None], cancel: threading.Event) -> None:
        try:
            body(cancel)
        except Cancelled:                # Cancel (or a reset) stopped it: nothing crashed
            self._full_due = True
        except Exception:  # noqa: BLE001 - a job must never leave the engine stuck as busy
            log.exception("job crashed")
            self._full_due = True
            self._notice = {"level": "problem", "text": "Something unexpected went wrong. Please try again."}
        finally:
            with self._lock:
                self._job = None
            self._publish()

    def cancel(self) -> None:
        with self._lock:
            if self._job:
                self._cancel.set()

    def _require_idle(self) -> None:
        if self._job:
            raise Conflict(f"Please wait: \"{self._job['label']}\" is still running.")

    # -- public operations -------------------------------------------------------------------------------------

    def set_mode(self, mode: str) -> None:
        with self._lock:
            self._require_idle()
            if mode == "live" and self._live_problem:
                raise Conflict(self._live_problem)
            if mode == self.mode:
                return
            self.mode, self._demo, self._notice = mode, None, None
            self._baseline, self._full_due = self._idle_baseline(), True
        if mode == "live":
            self._go_live()
        elif self._feeder is not None:
            self._feeder.stop()
        self._publish()

    def set_feeder(self, running: bool) -> None:
        with self._lock:
            if self.mode != "live":
                raise Conflict("The background data only runs in live mode.")
            self._get_feeder()
            self._feed_paused = not running
        self._publish()

    def prepare(self, scenario_id: str) -> None:
        sc = self._scenario(scenario_id)
        if self.mode == "replay":
            with self._lock:
                self._require_idle()
                self._open_demo(sc, "ready")
                self._notice = {"level": "info", "text": "Replay mode: this demo plays back a recording, "
                                                         "nothing is changed on the stack."}
            self._publish()
            return
        label = f"Getting ready: {sc.title}"

        def begin() -> None:
            self._start_job("prepare", sc.id, None, label, lambda cancel: self._prepare(sc, cancel))

        if self._wait_for_baseline(label, begin):
            with self._lock:
                self._open_demo(sc, "preparing")
                self._demo["message"] = WAITING
            self._publish()

    def run_step(self, scenario_id: str, step_id: str, submitted: dict[str, Any]) -> None:
        sc = self._scenario(scenario_id)
        step = next((s for s in sc.demo.steps if s.id == step_id), None)
        if step is None:
            raise BadRequest(f"This scenario has no step '{step_id}'.")
        with self._lock:
            self._require_idle()
            ready = self._demo and self._demo["scenario"] == sc.id and self._demo["status"] == "ready"
            if not ready:
                if self.mode == "live":
                    raise Conflict("Open the demo first: press its start button and wait until it is ready.")
                self._open_demo(sc, "ready")
            values = self._resolve_inputs(sc, step, submitted)
            self._chosen = values
        body = (lambda cancel: self._replay_step(sc, step, cancel)) if self.mode == "replay" else \
               (lambda cancel: self._live_step(sc, step, values, cancel))
        self._start_job("step", sc.id, step.id, step.title, body)

    def cleanup(self) -> None:
        if self.mode == "replay":
            with self._lock:
                self._require_idle()
                self._demo = None
                self._notice = {"level": "info", "text": "Replay mode: nothing was changed, so there is nothing "
                                                         "to clean up."}
            self._publish()
            return
        self._wait_for_baseline("Putting everything back", lambda: self._start_job(
            "cleanup", None, None, "Putting everything back", self._cleanup_job))

    def ensure_baseline(self) -> None:
        """Check (and where needed repair) the baseline in the background. After a cold start of the stack the
        studio is up before the database and the Airflows are: wait for the database first, and try again a few times
        when the check fails, instead of leaving "failed" on the home page until somebody opens a scenario."""
        def body(cancel: threading.Event) -> None:
            self._wait_for_database(cancel)
            self._forget_if_world_gone()
            for attempt in range(BASELINE_ATTEMPTS):
                if self._ensure_baseline(cancel):
                    return
                if attempt + 1 < BASELINE_ATTEMPTS and cancel.wait(BASELINE_RETRY_AFTER):
                    raise Cancelled()

        self._start_job("prepare", None, None, "Checking that the demo stack is ready", body)

    def _wait_for_database(self, cancel: threading.Event) -> None:
        """Until every environment's landing tables exist, at most DATABASE_WAIT seconds. The schema alone is not
        enough: on a first start the studio is up before bootstrap has created the tables. Returns either way: the
        baseline check that follows says what is wrong if it still cannot connect."""
        ready = getattr(getattr(self._get_feeder(), "writer", None), "landing_ready", None)
        if ready is None:
            return
        deadline, said = time.monotonic() + DATABASE_WAIT, False
        while True:
            try:
                if all(ready(env) for env in ENVIRONMENTS):
                    return
            except Exception:  # noqa: BLE001 - not up yet, whatever the driver calls it
                pass
            if time.monotonic() >= deadline:
                log.warning("the landing tables did not appear within %s seconds", int(DATABASE_WAIT))
                return
            if not said:
                said = True
                with self._lock:
                    self._baseline.update(status="preparing", detail="Waiting for the database to start.")
                self._publish()
            if cancel.wait(DATABASE_POLL):
                raise Cancelled()

    def _baseline_job_running(self) -> bool:
        job = self._job
        return bool(job and job["kind"] == "prepare" and job["scenario"] is None)

    def _wait_for_baseline(self, label: str, begin: Callable[[], None]) -> bool:
        """Run `begin` (it starts a job) now, or, while the baseline check runs, as soon as that has finished (at
        most BASELINE_WAIT seconds). Returns True when the request had to wait."""
        with self._lock:
            if not self._baseline_job_running():
                waiting = False
            elif self._queued:
                raise Conflict("Please wait: another request is already waiting for the practice system.")
            else:
                waiting, self._queued, self._notice = True, {"label": label, "text": WAITING}, None
                thread = self._thread
        if not waiting:
            begin()
            return False
        self._publish()
        threading.Thread(target=self._begin_after, args=(thread, begin), daemon=True, name="wait-baseline").start()
        return True

    def _begin_after(self, thread: threading.Thread | None, begin: Callable[[], None]) -> None:
        if thread is not None:
            thread.join(BASELINE_WAIT)
        late = thread is not None and thread.is_alive()
        with self._lock:
            dropped, self._queued = self._queued is None, None      # a reset drops what was waiting
        if dropped:
            return
        try:
            if late:
                raise Conflict("The practice system is taking longer than usual. Please try again in a minute.")
            begin()
        except Conflict as e:
            with self._lock:
                self._notice = {"level": "problem", "text": str(e)}
                if self._demo and self._demo["status"] == "preparing" and (late or not self._job):
                    self._demo.update(status="failed", message=str(e))
            self._publish()
        else:
            self._publish()

    def reset_state(self) -> None:
        """Forget everything the studio saved (what demos left behind, the feeder, the code baseline) and check the
        baseline again, whatever the stack looks like now. A running baseline check (the one from a cold start, which
        finds an empty stack) is stopped first; a running demo step or clean-up is not touched."""
        with self._lock:
            baseline_thread = self._thread if self._baseline_job_running() else None
            if baseline_thread is not None:
                self._cancel.set()
                self._queued = None          # a request that waits for that check must not start after it
        if baseline_thread is not None:
            baseline_thread.join(RESET_CANCEL_WAIT)
        with self._lock:
            if self._job:
                if self._baseline_job_running():
                    raise Conflict("Not now: the check of the demo stack is still stopping. Wait a moment, then run "
                                   "this again.")
                raise Conflict(f"Not now: \"{self._job['label']}\" is still running. Wait until it has finished "
                               "(or press Cancel in the app), then run this again.")
            self._forget_saved_state()
            self._demo, self._notice, self._chosen = None, None, {}
            self._baseline = self._idle_baseline()
        self._publish()
        if self.mode == "live":
            self.ensure_baseline()

    def _forget_saved_state(self) -> None:
        self._full_due = True
        self._store.clear()
        for name in ("git_baseline.json", "sql_undo.json"):
            (Path(self.settings.data_dir) / name).unlink(missing_ok=True)
        feeder = self._get_feeder() if self.mode == "live" else self._feeder
        if feeder is not None and hasattr(feeder, "forget"):
            feeder.forget()

    def _forget_if_world_gone(self) -> None:
        """After `make reset` the databases are empty again: what was saved describes a world that is gone, so it is
        forgotten (the baseline check then rebuilds everything). Needs every environment to have lost its data."""
        try:
            feeder = self._get_feeder()
            with ThreadPoolExecutor(max_workers=len(ENVIRONMENTS)) as pool:
                gone = not any(pool.map(feeder.is_seeded, ENVIRONMENTS))
        except Exception:  # noqa: BLE001 - an unreachable database is not a reset
            log.exception("could not tell whether the stack was reset")
            return
        data = Path(self.settings.data_dir)
        if gone and (self._store.items() or any((data / n).exists() for n in ("git_baseline.json", "sql_undo.json"))):
            log.info("every environment is empty: forgetting the saved state of an earlier world")
            with self._lock:
                self._forget_saved_state()
            self._publish()

    # -- demo state --------------------------------------------------------------------------------------------

    def _scenario(self, scenario_id: str) -> Scenario:
        try:
            return self.catalog.scenarios[scenario_id]
        except KeyError:
            raise BadRequest("There is no such scenario.") from None

    def today_in(self, env: str) -> Callable[[], date]:
        """The real "today" (the same for every environment), for `day(n)` in scenario files."""
        return lambda: self._feeder.today(env) if self._feeder is not None else date.today()

    def _template(self, sc: Scenario, values: dict[str, Any]) -> TemplateContext:
        return TemplateContext(sc.environment, values, self.today_in(sc.environment))

    def _defaults(self, sc: Scenario) -> dict[str, Any]:
        values: dict[str, Any] = {}
        for step in sc.demo.steps:
            for inp in step.inputs:
                rendered = render_input(inp, self._template(sc, values))
                values[inp.id] = rendered.coerce(rendered.default)
        return values

    def _resolve_inputs(self, sc: Scenario, step: Step, submitted: dict[str, Any]) -> dict[str, Any]:
        """The values for this run: defaults, earlier choices, and what was submitted, each checked."""
        unknown = set(submitted) - {i.id for i in step.inputs}
        if unknown:
            raise BadRequest("This step has no choice called " + ", ".join(sorted(unknown)) + ".")
        values = dict(self._chosen)
        for inp in step.inputs:
            rendered = render_input(inp, self._template(sc, values))
            try:
                values[inp.id] = rendered.coerce(submitted.get(inp.id, values[inp.id]))
            except ValueError as e:
                raise BadRequest(str(e)) from None
        return values

    def _shows(self, sc: Scenario, step: Step, values: dict[str, Any]) -> list[tuple[dict[str, Any], str]]:
        return [render_item(item, self._template(sc, values)) for item in step.show]

    def _open_demo(self, sc: Scenario, status: str) -> None:
        self._chosen = self._defaults(sc)
        self._demo = {"scenario": sc.id, "status": status, "message": "", "steps": [
            {"id": s.id, "status": "todo", "summary": "", "outcome": "", "actions": [],
             "evidence": [{**item, "data": None} for item, _ in self._shows(sc, s, self._chosen)]}
            for s in sc.demo.steps]}

    def _set_demo(self, status: str, message: str = "") -> None:
        with self._lock:
            if status == "failed":
                self._full_due = True
            if self._demo:
                self._demo.update(status=status, message=message)
        self._publish()

    def _step_state(self, step_id: str) -> dict[str, Any]:
        return next(s for s in self._demo["steps"] if s["id"] == step_id)

    def _reset_after(self, sc: Scenario, step_id: str) -> None:
        """Every step after `step_id` goes back to todo without evidence data, summary or outcome (the lock is held)."""
        later = False
        for state, step in zip(self._demo["steps"], sc.demo.steps):
            if later:
                state.update(status="todo", summary="", outcome="", actions=[],
                             evidence=[{**item, "data": None} for item, _ in self._shows(sc, step, self._chosen)])
                state.pop("outcome_kind", None)
            later = later or state["id"] == step_id

    # -- running actions ---------------------------------------------------------------------------------------

    def _ctx(self, env: str, cancel: threading.Event, progress: Callable[[str], None] = lambda text: None,
             last: Result | None = None) -> Ctx:
        return Ctx(env=env, settings=self.settings, feeder=self._feeder, cancel=cancel, progress=progress,
                   last_result=last, results=dict(self._results))

    @staticmethod
    def _params(spec: base.Spec, raw: dict[str, Any], env: str) -> Any:
        params = spec.params.model_validate(raw)
        if params.env is None:
            params.env = env
        return params

    def _touch(self, resources: set[str], scenario_id: str) -> None:
        if resources:
            with self._lock:
                self._store.add(resources, scenario_id)
            self._publish()

    def _run_items(self, sc: Scenario, items: list[dict[str, Any]], values: dict[str, Any],
                   cancel: threading.Event, entries: list[dict[str, Any]],
                   on_change: Callable[[], None]) -> Result | None:
        """Run action items in order. `entries` is the visible action list; it is updated as things happen.
        Raises Cancelled or StepFailed (every other error becomes a plain StepFailed)."""
        last: Result | None = None
        for raw in items:
            name = raw["action"]
            spec = base.ACTIONS[name]
            entry = {"label": ACTION_LABELS.get(name.partition(".")[0], "Working on the stack"),
                     "status": "running", "summary": "", "details": ""}
            try:
                item, env = render_item({k: v for k, v in raw.items() if k != "action"}, self._template(sc, values))
                params = self._params(spec, item, env)
                if spec.label:
                    entry["label"] = spec.label(params)
            except (TemplateError, ValidationError, ValueError) as e:
                entry.update(status="failed", summary="This step is not set up correctly.", details=str(e))
                self._append(entries, entry, on_change)
                raise StepFailed(entry["summary"], str(e)) from e
            self._append(entries, entry, on_change)

            def progress(text: str, entry: dict[str, Any] = entry) -> None:
                with self._lock:
                    entry["summary"] = names.plain_words(text)
                on_change()

            last = self._run_one(sc, spec, params, entry, self._ctx(env, cancel, progress, last), on_change)
            self._last_result = last
            self._results[name] = last
        return last

    def _append(self, entries: list[dict[str, Any]], entry: dict[str, Any], on_change: Callable[[], None]) -> None:
        with self._lock:
            entries.append(entry)
        on_change()

    def _run_one(self, sc: Scenario, spec: base.Spec, params: Any, entry: dict[str, Any], ctx: Ctx,
                 on_change: Callable[[], None]) -> Result:
        def finish(status: str, summary: str, details: str = "") -> None:
            with self._lock:
                entry.update(status=status, summary=summary, details=details)
            on_change()

        try:
            result = spec.run(ctx, params)
            result.summary, result.details = names.plain_pair(result.summary, result.details)
        except Cancelled as e:
            self._touch(set(getattr(e, "touched", ())), sc.id)
            finish("failed", "Stopped.")
            raise
        except StepFailed as e:
            e.summary, e.details = names.plain_pair(e.summary, e.details)
            self._touch(e.touched, sc.id)
            finish("failed", e.summary, e.details)
            raise
        except Exception as e:  # noqa: BLE001 - any action can break; the audience gets one plain sentence
            noun = NAMESPACES.get(spec.name.partition(".")[0], spec.name.partition(".")[0])
            summary, details = f"Something went wrong talking to {noun}.", traceback.format_exc()
            self._touch(set(getattr(e, "touched", ())), sc.id)   # what the action changed before it broke
            finish("failed", summary, details)
            raise StepFailed(summary, details) from e
        self._touch(result.touched, sc.id)
        finish("done", result.summary, result.details)
        return result

    # -- prepare / cleanup -------------------------------------------------------------------------------------

    def _prepare(self, sc: Scenario, cancel: threading.Event) -> None:
        with self._lock:
            self._open_demo(sc, "preparing")
        self._publish()
        began = time.monotonic()
        try:
            self._forget_if_world_gone()
            self._cleaned_envs = set()
            failed = self._clean_touched(cancel)
            if failed:
                self._set_demo("failed", "Something from an earlier demo could not be put back: "
                               + "; ".join(failed) + ".")
                return
            if not self._ensure_baseline(cancel, full=self._full_check_due(), recheck=set(self._cleaned_envs)):
                self._set_demo("failed", self._baseline["detail"])
                return
            self._run_items(sc, sc.demo.setup, self._chosen, cancel, self._job_actions, self._publish)
        except Cancelled:
            self._set_demo("failed", "Getting ready was stopped. Press the start button to try again.")
        except StepFailed as e:
            self._set_demo("failed", e.summary)
        else:
            self._set_demo("ready")
            try:
                self.recorder.save_prepare(sc.id, time.monotonic() - began)
            except OSError:
                log.exception("could not save the preparation time of %s", sc.id)

    def _full_check_due(self) -> bool:
        last = self._last_full
        return self._full_due or last is None or time.monotonic() - last > FULL_CHECK_EVERY

    def _cleanup_job(self, cancel: threading.Event) -> None:
        try:
            failed = self._clean_touched(cancel)
        except Cancelled:
            self._full_due = True
            self._notice = {"level": "info", "text": "Cleaning up was stopped. What is left is listed on the home page."}
        else:
            self._full_due = self._full_due or bool(failed)
            self._notice = {"level": "problem", "text": "Not everything could be put back: " + "; ".join(failed) + "."} \
                if failed else {"level": "info", "text": "Everything is back to normal."}
        with self._lock:
            self._demo = None

    def _clean_touched(self, cancel: threading.Event) -> list[str]:
        """Undo the touched set, cheapest first. A resource whose cleaner fails stays in the set; returns the plain
        labels of those."""
        failed: list[str] = []
        for resource in sorted(self._store.items(), key=lambda r: (_clean_rank(r), r)):
            if cancel.is_set():
                raise Cancelled()
            label = leftover_label(resource)
            entry = {"label": f"Putting back: {label}", "status": "running", "summary": "", "details": ""}
            self._append(self._job_actions, entry, self._publish)
            try:
                result = self._clean_one(resource, cancel)
            except Cancelled:
                entry.update(status="failed", summary="Stopped.")
                raise
            except StepFailed as e:
                entry.update(status="failed", **dict(zip(("summary", "details"), names.plain_pair(e.summary, e.details))))
                failed.append(label)
            else:
                self._store.remove(resource)
                if resource.partition(":")[0] in ("version", "variable"):
                    self._cleaned_envs.add(self._env_of(resource))
                entry.update(status="done", **dict(zip(("summary", "details"),
                                                       names.plain_pair(result.summary, result.details))))
            self._publish()
        return failed

    def _env_of(self, resource: str) -> str:
        """The environment a touched resource lives in (the owning scenario's, when the name has none)."""
        rest = resource.partition(":")[2]
        if rest.split(":")[0] in ENVIRONMENTS:
            return rest.split(":")[0]
        owner = self._store.items().get(resource)
        return self.catalog.scenarios[owner].environment if owner in self.catalog.scenarios else "dev"

    def _clean_one(self, resource: str, cancel: threading.Event) -> Result:
        prefix, _, rest = resource.partition(":")
        cleaner = base.CLEANERS.get(prefix)
        if cleaner is None:
            raise StepFailed("There is no way to put this back automatically.", f"no cleaner for '{prefix}'")
        env = self._env_of(resource)
        try:
            return cleaner(self._ctx(env, cancel), resource)
        except (Cancelled, StepFailed):
            raise
        except Exception as e:  # noqa: BLE001
            raise StepFailed("Something went wrong while putting this back.", traceback.format_exc()) from e

    # -- baseline ----------------------------------------------------------------------------------------------

    def _provider(self, name: str, env: str, cancel: threading.Event, extra: dict[str, Any] | None = None) -> Any:
        spec = base.EVIDENCE.get(name)
        if spec is None:
            raise StepFailed(f"The stack cannot be checked: '{name}' is not available.")
        return spec.run(self._ctx(env, cancel), self._params(spec, extra or {}, env))

    def _act(self, name: str, env: str, cancel: threading.Event, raw: dict[str, Any]) -> Result:
        spec = base.ACTIONS[name]
        return spec.run(self._ctx(env, cancel), self._params(spec, raw, env))

    @staticmethod
    def _tag_of(data: dict[str, Any], env: str) -> str | None:
        """The image tag the `versions` evidence reports for `env`."""
        return next((r.get("tag") for r in data.get("environments", []) if r.get("env") == env), None)

    def _built_once(self, env: str, cancel: threading.Event) -> bool:
        """Do the environment's tables exist? Falls back to "a run is on record" for a feeder without that check."""
        try:
            built = getattr(self._get_feeder(), "tables_built", None)
            if built is not None:
                return bool(built(env))
            return bool(self._provider("run_history", env, cancel, {"limit": 1}).get("runs"))
        except pymysql.MySQLError:
            return False

    def _baseline_checks(self, cancel: threading.Event, envs: tuple[str, ...] = ENVIRONMENTS) -> list[list[Check]]:
        """The requirements per environment, in the order they must be fixed (data, version, tables)."""
        feeder, tag = self._get_feeder(), self.settings.baseline_tag
        wiped: set[str] = set()   # environments whose data had to be written again: the world was reset

        def seed(env: str) -> None:
            wiped.add(env)
            feeder.seed(env, days=BASELINE_DAYS)

        def has_version(env: str) -> bool:
            # an environment that lost its data lost its code and documentation too: deploy the version again
            return env not in wiped and bool(self._tag_of(self._provider("versions", env, cancel, {"only": env}), env))

        return [[
            Check(f"Demo data in {env}", lambda e=env: feeder.is_seeded(e), lambda e=env: seed(e)),
            Check(f"A numbered version running in {env}", lambda e=env: has_version(e),
                  lambda e=env: self._act("deploy.set_version", e, cancel, {"tag": tag})),
            Check(f"Tables built once in {env}", lambda e=env: self._built_once(e, cancel),
                  lambda e=env: self._act("dbt.run", e, cancel, {"as": "service", "command": "build"})),
        ] for env in envs]

    def _run_checks(self, checks: list[Check], items: list[dict[str, Any]]) -> str | None:
        """One environment's checks, one after the other. Returns the reason of the first failure, or None."""
        for check, item in zip(checks, items):
            item["status"] = "running"
            self._publish()
            try:
                if not check.ok():
                    check.fix()
            except Cancelled:
                item["status"] = "failed"
                raise
            except Exception as e:  # noqa: BLE001
                log.exception("baseline check failed: %s", check.label)
                item["status"] = "failed"
                return f"Could not get this ready: {check.label}. " + \
                    (e.summary if isinstance(e, StepFailed) else "Something went wrong.")
            item["status"] = "done"
        return None

    def _quick_ready(self, envs: tuple[str, ...]) -> set[str]:
        """The environments whose data and built tables are there, from one read-only SQL round trip each (at the
        same time). An environment that cannot be asked, or that lacks something, is not in the result."""
        quick = getattr(self._get_feeder(), "quick_ready", None)
        if quick is None:
            return set()

        def one(env: str) -> bool:
            try:
                return bool(quick(env))
            except Exception:  # noqa: BLE001 - the full check will find out what is wrong
                log.exception("quick check of %s failed", env)
                return False

        with ThreadPoolExecutor(max_workers=len(envs)) as pool:
            return {env for env, ok in zip(envs, pool.map(one, envs)) if ok}

    def _ensure_baseline(self, cancel: threading.Event, full: bool = True, recheck: set[str] | None = None) -> bool:
        """Make sure every environment has data, a deployed version and built tables. `full` does the whole reality
        check; otherwise only a quick check runs, and only the environments in `recheck` (a version or setting was
        just put back there) or that fail the quick check get the full check and repair. The environments are checked
        at the same time; each check shows its own progress. Returns False (with a plain reason in the baseline
        detail) when something could not be fixed."""
        envs = tuple(ENVIRONMENTS)
        if not full:
            ready = self._quick_ready(envs)
            envs = tuple(e for e in ENVIRONMENTS if e not in ready or e in (recheck or ()))
            if not envs:
                with self._lock:
                    self._baseline.update(status="ready", detail="Every environment has data, a version and built tables.")
                self._publish()
                return True
        try:
            chains = self._baseline_checks(cancel, envs)
        except Exception:  # noqa: BLE001
            log.exception("baseline checks could not be set up")
            return self._baseline_failed("The demo stack cannot be reached right now.")
        progress = [[{"label": c.label, "status": "todo"} for c in chain] for chain in chains]
        with self._lock:
            self._baseline = {"status": "preparing", "detail": "Checking the demo stack.",
                              "progress": [item for chain in progress for item in chain]}
        self._publish()
        with ThreadPoolExecutor(max_workers=len(chains)) as pool:
            futures = [pool.submit(self._run_checks, chain, items) for chain, items in zip(chains, progress)]
        problems, stopped = [], False
        for future in futures:
            try:
                problem = future.result()
            except Cancelled:
                stopped = True
                continue
            if problem:
                problems.append(problem)
        if stopped:
            self._baseline_failed("Checking the demo stack was stopped.")
            raise Cancelled()
        if problems:
            return self._baseline_failed(problems[0])
        with self._lock:
            if len(envs) == len(ENVIRONMENTS):
                self._full_due, self._last_full = False, time.monotonic()
            self._baseline.update(status="ready", detail="Every environment has data, a version and built tables.")
        self._publish()
        return True

    def _baseline_failed(self, detail: str) -> bool:
        with self._lock:
            self._full_due = True
            self._baseline.update(status="failed", detail=detail)
        self._publish()
        return False

    # -- live steps --------------------------------------------------------------------------------------------

    def _live_step(self, sc: Scenario, step: Step, values: dict[str, Any], cancel: threading.Event) -> None:
        shows = self._shows(sc, step, values)
        with self._lock:
            state = self._step_state(step.id)
            state.update(status="running", summary="", outcome="", actions=[],
                         evidence=[{**item, "data": None} for item, _ in shows])
            state.pop("outcome_kind", None)
            self._reset_after(sc, step.id)
            self._demo["status"] = "running"
            self._last_result = None
            self._results = {}
        collector = FrameCollector()
        began, started = time.monotonic(), datetime.now(timezone.utc)
        items = [(item["type"], item.get("title")) for item, _ in shows]

        def on_change() -> None:
            with self._lock:
                collector.frame(state["actions"], [e["data"] for e in state["evidence"]], items)
            self._publish()

        def refresh() -> None:
            for i, (item, env) in enumerate(shows):
                data = self._read_evidence(item, env, cancel, started)
                with self._lock:
                    state["evidence"][i]["data"] = data
            on_change()

        stop_loop = threading.Event()

        def loop() -> None:
            while not stop_loop.wait(EVIDENCE_EVERY):
                refresh()

        refresh()
        reader = threading.Thread(target=loop, daemon=True, name="evidence")
        reader.start()
        try:
            self._run_items(sc, step.do, values, cancel, state["actions"], on_change)
            status, summary = "done", self._last_result.summary if self._last_result else ""
        except Cancelled:
            status, summary = "stopped", STOPPED
        except StepFailed as e:
            status, summary = "failed", e.summary
        finally:
            stop_loop.set()
            reader.join()
        refresh()
        outcome, kind = "", None
        if status == "done":
            outcome, kind = self._outcome(sc, step, values, cancel, state, on_change)
        with self._lock:
            state.update(status=status, summary=summary, outcome=outcome)
            if kind:
                state["outcome_kind"] = kind
            else:
                state.pop("outcome_kind", None)
            if status != "done":
                self._full_due = True
                self._reset_after(sc, step.id)
            self._demo["status"] = "ready"
        on_change()
        if status == "done":
            self._record(sc, step, values, collector, summary, outcome, kind or "good", time.monotonic() - began)

    def _outcome(self, sc: Scenario, step: Step, values: dict[str, Any], cancel: threading.Event,
                 state: dict[str, Any], on_change: Callable[[], None]) -> tuple[str, str]:
        """The closing sentence and its kind (`good` | `not_quite`). A plain outcome is always good; an outcome with
        a check reads one number first. A check that cannot run keeps the step done but says it was not checked."""
        ctx = self._template(sc, values)
        if isinstance(step.outcome, str):
            return self._sentence(step.outcome, ctx), "good"
        try:
            value = self._check_number(str(render(step.outcome.check, ctx)))
            good = step.outcome.is_good(value)
            return self._sentence(step.outcome.good if good else step.outcome.not_quite, ctx.with_value(value)), \
                "good" if good else "not_quite"
        except Exception as e:  # noqa: BLE001 - the step itself worked; only its closing sentence is affected
            log.exception("outcome check of %s/%s failed", sc.id, step.id)
            details = e.details if isinstance(e, StepFailed) and e.details else f"{type(e).__name__}: {e}"
            entry = {"label": "Checking the result", "status": "failed", "summary": "The result could not be checked.",
                     "details": details}
            self._append(state["actions"], entry, on_change)
            return CHECK_FAILED, "not_quite"

    @staticmethod
    def _sentence(text: str, ctx: TemplateContext) -> str:
        try:
            return str(render(text, ctx))
        except TemplateError:
            return text

    def _check_number(self, query: str) -> int | float:
        """The one number a check query returns (read-only, as root, `CHECK_TIMEOUT` seconds)."""
        _, rows = clients.read_rows(self.settings, query, limit=1, timeout=CHECK_TIMEOUT)
        value = rows[0][0] if rows and rows[0] else None
        if isinstance(value, bool) or not isinstance(value, (int, float, Decimal)):
            raise ValueError(f"the check returned {value!r}, not a number")
        number = float(value)
        return int(number) if number == int(number) else round(number, 2)

    def _read_evidence(self, item: dict[str, Any], env: str, cancel: threading.Event,
                       step_started: datetime | None = None) -> dict[str, Any]:
        """One evidence item's data; a provider that fails only marks its own item. Of the runs in `run_history`
        the ones started since the current step began are flagged `current`."""
        spec = base.EVIDENCE[item["type"]]
        try:
            params = self._params(spec, {k: v for k, v in item.items() if k not in ("type", "before_of")}, env)
            data = spec.run(self._ctx(env, cancel, last=self._last_result), params)
            if item["type"] == "run_history" and step_started is not None:
                self._flag_current(data, step_started)
            return data
        except Exception:  # noqa: BLE001
            log.exception("evidence '%s' failed", item["type"])
            return {"error": EVIDENCE_ERROR}

    @staticmethod
    def _flag_current(data: dict[str, Any], step_started: datetime) -> None:
        for run in data.get("runs") or []:
            try:
                at = datetime.fromisoformat(str(run["started"]))
            except (KeyError, ValueError):
                continue
            if at.tzinfo is None:
                at = at.replace(tzinfo=timezone.utc)
            if at >= step_started - RUN_CLOCK_SLACK:
                run["current"] = True

    def _record(self, sc: Scenario, step: Step, values: dict[str, Any], collector: FrameCollector,
                summary: str, outcome: str, outcome_kind: str, duration: float) -> None:
        own = {i.id: values[i.id] for i in step.inputs}
        try:
            self.recorder.save_step(sc.id, step.id, own, collector.frames, summary, outcome, outcome_kind,
                                    duration, [item["type"] for item in step.show])
        except OSError:
            log.exception("could not save the recording of %s/%s", sc.id, step.id)

    # -- replay ------------------------------------------------------------------------------------------------

    def _replay_step(self, sc: Scenario, step: Step, cancel: threading.Event) -> None:
        recording = self.recorder.load_matching(sc.id, step.id, [item["type"] for item in step.show])
        with self._lock:
            state = self._step_state(step.id)
            state.update(status="running", summary="", outcome="", actions=[])
            state.pop("outcome_kind", None)
            self._reset_after(sc, step.id)
        self._publish()
        if recording is None:
            with self._lock:
                state.update(status="failed", summary="This step has not been recorded yet.")
            return

        def apply(frame: dict[str, Any]) -> None:
            with self._lock:
                state["actions"] = copy.deepcopy(frame["actions"])
                for item, shown in zip(state["evidence"], frame["evidence"]):
                    item["data"] = copy.deepcopy(shown["data"])
                    item["recorded_at"] = recording["recorded_at"]
            self._publish()

        try:
            replay(recording["frames"], apply, cancel)
        except Cancelled:
            with self._lock:
                state.update(status="stopped", summary=STOPPED)
            return
        with self._lock:
            state.update(status="done", summary=recording["summary"], outcome=recording["outcome"],
                         outcome_kind=recording["outcome_kind"])

    # -- stack strip -------------------------------------------------------------------------------------------

    def _stack_loop(self) -> None:
        while not self._stopping.is_set():
            self._refresh_stack()
            self._stopping.wait(STACK_EVERY)

    def _refresh_stack(self) -> None:
        if self.mode != "live" or "stack" not in base.EVIDENCE:
            stack: list[dict[str, Any]] = []
        else:
            try:
                data = self._provider("stack", "dev", threading.Event())
                stack = data if isinstance(data, list) else data.get("services", [])
            except Exception:  # noqa: BLE001
                log.exception("stack status could not be read")
                return
        with self._lock:
            changed, self._stack = stack != self._stack, stack
        if changed:
            self._publish()
