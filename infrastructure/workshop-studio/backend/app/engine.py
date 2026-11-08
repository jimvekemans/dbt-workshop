"""Scenario engine: world state machine for the Sim/Live studio.

State is plain JSON (persisted to state.json). `sim:` effects always animate the map; in Live mode the `live:`
actions additionally run against the stack and probe results ("observed") override the simulated health."""
from __future__ import annotations

import json
import os
import threading
import time
from collections import deque
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

from . import actions
from .actions.base import ActionError, Cancelled, Ctx, PreconditionFailed
from .config import Settings
from .schema import HEALTH_RANK, Effects, LiveAction, Scenario, World

ChannelFaultDefaults = {"enabled": True, "delay_batches": 0, "duplicate_pct": 0, "partial_pct": 100,
                        "null_key_pct": 0, "schema_variant": "v1", "late_pct": 0, "bot_events": 0}
PERSISTED = ("mode", "dry_run", "sprint_day", "scenario_id", "phase", "step_index", "touched", "ripple",
             "pre_inject", "live_undo", "probe_since")


class Busy(RuntimeError):
    pass


def utc_now() -> str:
    """UTC wall clock in the format dbt writes run_started_at (probe fence after a live Reset)."""
    return datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S")


def undo_actions(sc: Scenario) -> list[dict]:
    """What a scenario changes in the stack, as actions that put it back - clearing or replacing the scenario runs them,
    so its faults do not linger:
      set_channel       -> exactly the fields it set, back to healthy
      airflow_variable  -> the lever Variable deleted again (none of them exists in a fresh stack)
      sql with `undo:`  -> that statement (e.g. remove the QA smoke-test orders)"""
    fields: dict[tuple[str, str], dict] = {}
    other: list[dict] = []
    for a in sc.live_actions():
        p = a.params()
        if a.action == "set_channel":
            for f, default in ChannelFaultDefaults.items():
                if f in p or p.get("field") == f:
                    fields.setdefault((p["env"], p["channel"]), {})[f] = default
        elif a.action == "airflow_variable" and not p.get("delete"):
            other.append({"action": "airflow_variable", "env": p["env"], "key": p["key"], "delete": True})
        elif a.action == "sql" and p.get("undo"):
            undo = {"action": "sql", "env": p["env"], "statement": p["undo"]}
            if p.get("undo_ignore_errors"):     # e.g. 1917: the step that fixed it already re-granted the group
                undo["ignore_errors"] = list(p["undo_ignore_errors"])
            other.append(undo)
    channels = [{"action": "set_channel", "env": env, "channel": ch, **vals} for (env, ch), vals in fields.items()]
    return _merge_undo([], channels + other)


def _undo_key(a: dict) -> tuple:
    if a["action"] == "set_channel":
        return ("set_channel", a["env"], a["channel"])
    if a["action"] == "airflow_variable":
        return ("airflow_variable", a["env"], a["key"])
    return (a["action"], a["env"], a.get("statement", ""))


def _merge_undo(old: list[dict], new: list[dict]) -> list[dict]:
    merged: dict[tuple, dict] = {}
    for a in [*old, *new]:
        cur = merged.setdefault(_undo_key(a), {})
        cur.update(a)   # set_channel: the union of the fields; the others: one entry each
    return list(merged.values())


def undo_label(a: dict) -> str:
    """How a pending undo shows in the header pill: kafka@prod, disabled_dbt_models@prod, sql@dev."""
    what = a.get("channel") or a.get("key") or a["action"]
    return f"{what}@{a['env']}"


class Engine:
    def __init__(self, settings: Settings, world: World, scenarios: dict[str, Scenario]):
        self.settings = settings
        self.world = world
        self.scenarios = scenarios
        self.node_defs = {n.id: n for n in world.nodes}
        self.listeners: list[Callable[[dict], None]] = []
        self.probe_now: Callable[[], None] | None = None
        self.cache: dict[str, Any] = {}
        self._mut = threading.RLock()
        self._op = threading.Lock()
        self._cancel = threading.Event()   # Cancel button: long live waits (CI, Airflow, tags) stop at their next poll
        self.content_version = 0           # bumps when the scenario YAML is reloaded (the UI refetches the world)
        self.log_lines: deque[dict] = deque(maxlen=400)
        self.state: dict[str, Any] = self._fresh_state()
        self._load()

    # ------------------------------------------------------------------ state
    def _fresh_state(self, mode: str = "sim") -> dict[str, Any]:
        nodes = {}
        for n in self.world.nodes:
            nodes[n.id] = {"health": "idle", "note": "", "metrics": dict(n.metrics),
                           "knobs": {k.key: k.default for k in n.knobs}}
        return {"mode": mode, "dry_run": self.settings.dry_run, "sprint_day": 0, "scenario_id": None,
                "phase": "idle", "step_index": 0, "nodes": nodes, "observed": {}, "ripple": {},
                "touched": [], "busy": None, "error": None, "probes": {}, "updated": time.time(),
                # node state captured right before the active scenario changed anything (clear/replace restores it)
                "pre_inject": {},
                # set_channel actions that undo the channel faults a live injection left in the stack
                "live_undo": [],
                # probes ignore dbt results / Airflow runs from before the last live Reset
                "probe_since": None}

    def _load(self) -> None:
        path = self.settings.state_file
        try:
            data = json.loads(Path(path).read_text())
            fresh = self._fresh_state(data.get("mode", "sim"))
            for k in PERSISTED:
                if k in data:
                    fresh[k] = data[k]
            for nid, nd in data.get("nodes", {}).items():
                if nid in fresh["nodes"]:
                    fresh["nodes"][nid].update(nd)
            if fresh["scenario_id"] not in (None, *self.scenarios):
                fresh["scenario_id"], fresh["phase"] = None, "idle"
            self.state = fresh
        except (OSError, ValueError):
            pass

    def _save(self) -> None:
        """Serialised under the state lock (the probe thread publishes while operations run) and written atomically,
        so a crash or a concurrent save never leaves a torn state.json behind."""
        try:
            with self._mut:
                text = json.dumps({k: v for k, v in self.state.items() if k not in ("observed", "busy", "probes", "error")})
            path = Path(self.settings.state_file)
            path.parent.mkdir(parents=True, exist_ok=True)
            tmp = path.with_name(f".{path.name}.{threading.get_ident()}.tmp")
            tmp.write_text(text)
            os.replace(tmp, path)
        except OSError:
            pass  # persistence is a convenience; never break the show

    def effective_health(self, nid: str) -> str:
        st = self.state
        if st["mode"] == "live" and nid in st["observed"] and st["observed"][nid].get("health"):
            return st["observed"][nid]["health"]
        return st["nodes"][nid]["health"]

    def snapshot(self) -> dict[str, Any]:
        with self._mut:
            st = self.state
            nodes = {}
            for nid, n in st["nodes"].items():
                obs = st["observed"].get(nid, {}) if st["mode"] == "live" else {}
                nodes[nid] = {"health": self.effective_health(nid), "simHealth": n["health"],
                              "note": obs.get("note") or n["note"], "metrics": {**n["metrics"], **obs.get("metrics", {})},
                              "knobs": n["knobs"], "observed": bool(obs)}
            counts = {h: sum(1 for v in nodes.values() if v["health"] == h) for h in HEALTH_RANK}
            sc = self.scenarios.get(st["scenario_id"]) if st["scenario_id"] else None
            return {"mode": st["mode"], "dryRun": st["dry_run"], "sprintDay": st["sprint_day"],
                    "sprintLength": len(self.world.sprint.days), "scenarioId": st["scenario_id"], "phase": st["phase"],
                    "stepIndex": st["step_index"], "stepCount": len(sc.steps) if sc else 0, "nodes": nodes,
                    "ripple": st["ripple"], "counts": counts, "busy": st["busy"], "error": st["error"],
                    "probes": st["probes"], "touched": st["touched"], "contentVersion": self.content_version,
                    "liveFaultsPending": [undo_label(a) for a in st.get("live_undo") or []]}

    def emit(self, event: dict) -> None:
        for fn in list(self.listeners):
            try:
                fn(event)
            except Exception:  # noqa: BLE001 - a dead websocket must not kill the engine
                pass

    def publish(self) -> None:
        with self._mut:
            self.state["updated"] = time.time()
        self._save()
        self.emit({"type": "state", "state": self.snapshot()})

    def log(self, msg: str, level: str = "info") -> None:
        entry = {"t": time.strftime("%H:%M:%S"), "level": level, "msg": msg}
        self.log_lines.append(entry)
        self.emit({"type": "log", "entry": entry})

    # ---------------------------------------------------------------- effects
    def apply_effects(self, effects: Effects, *, track: bool = True) -> None:
        """track=False for sprint days: only what a scenario changes is rolled back when it is cleared."""
        with self._mut:
            for nid, e in effects.items():
                n = self.state["nodes"][nid]
                if e.health:
                    n["health"] = e.health
                    if e.note is None:
                        n["note"] = ""
                if e.note is not None:
                    n["note"] = e.note
                n["metrics"].update(e.metrics)
                n["knobs"].update(e.knobs)
                if track and nid not in self.state["touched"]:
                    self.state["touched"].append(nid)

    def _capture(self) -> dict[str, dict]:
        with self._mut:
            return {nid: {"health": n["health"], "note": n["note"], "knobs": dict(n["knobs"]), "metrics": dict(n["metrics"])}
                    for nid, n in self.state["nodes"].items()}

    def _restore_scenario_map(self) -> None:
        """Put every node the active scenario touched back to how it was right before the injection (health, note,
        knobs, metrics), then forget the scenario. Nodes it did not touch keep their state (sprint progress)."""
        pre = self.state.get("pre_inject") or {}
        with self._mut:
            for nid in self.state["touched"]:
                node = self.state["nodes"].get(nid)
                if node is None:
                    continue
                if nid in pre:
                    node.update(health=pre[nid]["health"], note=pre[nid]["note"],
                                knobs=dict(pre[nid]["knobs"]), metrics=dict(pre[nid]["metrics"]))
                else:   # state.json from before pre_inject existed
                    node.update(health="ok", note="")
            self.state.update(scenario_id=None, phase="idle", step_index=0, ripple={}, touched=[], pre_inject={})

    def _undo_live_faults(self) -> None:
        """Run the pending undo actions of a live injection (Live mode only; dry-run just renders them)."""
        pending = self.state.get("live_undo") or []
        if not pending:
            return
        where = ", ".join(undo_label(a) for a in pending)
        if self.state["mode"] != "live":
            self.log(f"Faults injected live are still set in the stack ({where}): switch to Live and clear "
                     f"the scenario, or press Reset", "warn")
            return
        self._run_live([LiveAction(**a) for a in pending])
        if not self.state["dry_run"]:
            self.state["live_undo"] = []
            self.log(f"Live faults undone ({where})")

    def _ripple(self, sources: list[str]) -> dict[str, int]:
        """Downstream BFS depth from the injected nodes - the frontend staggers the impact animation with it."""
        adj: dict[str, list[str]] = {}
        for e in self.world.edges:
            adj.setdefault(e.source, []).append(e.target)
        depth = {s: 0 for s in sources}
        queue = deque(sources)
        while queue:
            cur = queue.popleft()
            for nxt in adj.get(cur, []):
                if nxt not in depth:
                    depth[nxt] = depth[cur] + 1
                    queue.append(nxt)
        return depth

    def _baseline(self) -> None:
        """Injecting before/while the sprint runs: start from the steady state (everything running = ok)."""
        with self._mut:
            for n in self.state["nodes"].values():
                if n["health"] == "idle":
                    n["health"] = "ok"

    # ---------------------------------------------------------------- running
    def _ctx(self) -> Ctx:
        return Ctx(settings=self.settings, dry_run=self.state["dry_run"], log=lambda line: self.log(line), cache=self.cache,
                   cancel=self._cancel)

    def _run_live(self, acts: list[LiveAction]) -> None:
        if self.state["mode"] != "live":
            return
        ctx = self._ctx()
        for a in acts:
            ctx.check_cancel()
            res = actions.execute(a.action, a.params(), ctx)
            if res.output and not ctx.dry_run:
                # ok=False: a failure the step allows (allow_failure), e.g. a daily load that trips a test on purpose
                self.log(("✓ " if res.ok else "⚠ ") + res.output, "info" if res.ok else "warn")

    def _check_requires(self, sc: Scenario) -> None:
        """Live only: every precondition is checked (read-only) before anything runs, and all unmet ones are reported
        together. Sim mode needs nothing from the stack."""
        if self.state["mode"] != "live" or not sc.requires:
            return
        ctx = self._ctx()
        unmet: list[str] = []
        for a in sc.requires:
            try:
                actions.execute(a.action, a.params(), ctx)
            except PreconditionFailed as e:
                unmet.append(str(e))
        if unmet:
            raise PreconditionFailed(f"{sc.id} cannot start yet: " + " | ".join(unmet))

    def _operation(self, label: str):
        engine = self

        class _Op:
            def __enter__(self_inner):
                if not engine._op.acquire(blocking=False):
                    raise Busy(engine.state["busy"] or "another operation is running")
                engine._cancel.clear()
                with engine._mut:
                    engine.state["busy"], engine.state["error"] = label, None
                engine.publish()

            def __exit__(self_inner, exc_type, exc, tb):
                with engine._mut:
                    engine.state["busy"] = None
                    if isinstance(exc, Cancelled):
                        engine.state["error"] = f"{label}: cancelled"
                        engine.log(f"✗ {label}: cancelled. Live actions that already finished stay done; the map and "
                                   f"step are unchanged, so running it again repeats the whole step.", "warn")
                    elif exc and not isinstance(exc, Busy):
                        engine.state["error"] = str(exc)
                        engine.log(f"✗ {label}: {exc}", "error")
                engine._cancel.clear()
                engine._op.release()
                engine.publish()
                return False

        return _Op()

    def cancel(self) -> dict:
        """Stop the running operation at its next wait point (CI/tag/Airflow polls, or between two live actions).
        Deliberately not an operation itself: it must work while one holds the lock."""
        label = self.state["busy"]
        if not label:
            raise ValueError("nothing is running")
        self._cancel.set()
        self.log(f"Cancel requested: {label} stops at its next wait point", "warn")
        return self.snapshot()

    # ------------------------------------------------------------- public API
    def set_mode(self, mode: str) -> dict:
        if mode not in ("sim", "live"):
            raise ValueError("mode must be sim|live")
        with self._operation(f"switching to {mode}"):
            self.state["mode"] = mode
            self.log(f"Mode: {mode.upper()}" + (" (dry-run: commands are rendered, nothing is executed)" if mode == "live" and self.state["dry_run"] else ""))
        return self.snapshot()

    def set_dry_run(self, value: bool) -> dict:
        with self._operation("dry-run toggle"):
            self.state["dry_run"] = bool(value)
        return self.snapshot()

    def reset(self) -> dict:
        with self._operation("reset"):
            mode, dry = self.state["mode"], self.state["dry_run"]
            live = mode == "live" and not dry
            fence = utc_now()
            self._run_live([LiveAction(action="reset")])   # live first: a failed reset leaves the map as it was
            fresh = self._fresh_state(mode)
            fresh["dry_run"] = dry
            # a live reset cleans the stack: earlier test results / DAG runs no longer describe it, faults are gone
            fresh["probe_since"] = fence if live else self.state.get("probe_since")
            fresh["live_undo"] = [] if live else self.state.get("live_undo", [])
            self.state = fresh
            self.log("Reset: back to sprint day 0" + (" (probes ignore results from before this reset)" if live else ""))
        return self.snapshot()

    def milestone(self, op: str, name: str) -> dict:
        """Live: create or restore a SingleStore milestone (whole warehouse; SingleStore restarts). Restoring is a
        warehouse-only reset: GitLab, the deployed code and the Airflow levers stay as they are, the map too."""
        if op not in ("create", "restore", "delete"):
            raise ValueError(f"milestone op must be create|restore|delete, got {op!r}")
        if self.state["mode"] != "live" or self.state["dry_run"]:
            raise ValueError("milestones need Live mode without dry-run")
        with self._operation(f"milestone {op} {name}"):
            self._run_live([LiveAction(action="milestone", op=op, name=name)])
            if op == "restore":
                # the run log is back at the milestone: results recorded after it no longer describe the warehouse
                self.state["probe_since"] = utc_now()
            self.log(f"Milestone {op}: {name}")
        return self.snapshot()

    def milestones(self) -> list[dict[str, str]]:
        """[{name, created, size}] from singlestore/milestones.sh list; [] when the stack is not reachable."""
        if self.state["mode"] != "live" or self.state["dry_run"]:
            return []
        try:
            res = actions.execute("milestone", {"op": "list"}, self._ctx())
        except actions.ActionError:
            return []
        out = []
        for line in res.data.get("stdout", "").splitlines():
            parts = line.split()
            if len(parts) >= 5 and not line.startswith("("):     # name  YYYY-MM-DD HH:MM:SS UTC  size
                out.append({"name": parts[0], "created": " ".join(parts[1:4]), "size": parts[-1]})
        return out

    def tick(self) -> dict:
        with self._operation("sprint tick"):
            day = self.state["sprint_day"]
            days = self.world.sprint.days
            if day >= len(days):
                self.log("Sprint is over - reset to start a new one")
                return self.snapshot()
            spec = days[day]
            self.log(f"Sprint day {spec.day}: {spec.title}" + (f"  ⚑ {spec.milestone}" if spec.milestone else ""))
            self._run_live(spec.live)          # live first: a failing action leaves the map unchanged
            if self.state["mode"] == "live" and spec.live_note:
                self.log(f"Live: {spec.live_note}", "warn")
            self.state["sprint_day"] = day + 1
            self.apply_effects(spec.sim, track=False)
        return self.snapshot()

    def set_knob(self, node: str, key: str, value: Any) -> dict:
        nd = self.node_defs.get(node)
        knob = next((k for k in nd.knobs if k.key == key), None) if nd else None
        if not knob:
            raise KeyError(f"{node} has no knob {key!r}")
        if knob.type == "bool":
            value = bool(value)
        elif knob.type == "int":
            value = int(value)
            value = max(knob.min if knob.min is not None else value, min(knob.max if knob.max is not None else value, value))
        elif knob.options and value not in knob.options:
            raise ValueError(f"{key} must be one of {knob.options}")
        with self._operation(f"knob {node}.{key}"):
            if self.state["mode"] == "live" and knob.live:   # live first: if SingleStore refuses, the knob keeps its value
                self._run_live([LiveAction(action=knob.live.action, **{**knob.live.params(), "value": value})])
            self.state["nodes"][node]["knobs"][key] = value
            self.log(f"Knob {node}.{key} = {value}")
            if self.state["mode"] == "sim":
                self._sim_knob_health(node)
        return self.snapshot()

    def _sim_knob_health(self, node: str) -> None:
        nd = self.node_defs[node]
        if nd.kind != "channel":
            return
        knobs = self.state["nodes"][node]["knobs"]
        faulty = [k for k, d in ChannelFaultDefaults.items() if k in knobs and knobs[k] != d]
        n = self.state["nodes"][node]
        if "enabled" in faulty:
            n["health"], n["note"] = "fail", "channel disabled"
        elif faulty:
            n["health"], n["note"] = "warn", "fault injected: " + ", ".join(f"{k}={knobs[k]}" for k in faulty)
        else:
            n["health"], n["note"] = "ok", ""

    def inject(self, scenario_id: str) -> dict:
        sc = self.scenarios[scenario_id]
        with self._operation(f"inject {sc.id}"):
            self.log(f"Inject {sc.id}: {sc.title}")
            self._check_requires(sc)           # read-only: a wrong starting state stops here, before anything runs
            previous = self.state["scenario_id"]
            if previous:
                # the previous scenario's faults and map changes must not leak into this one
                self.log(f"Replacing {previous}: its channel faults are undone and its map changes rolled back", "warn")
                self._undo_live_faults()
                self._restore_scenario_map()
            self.log(sc.inject.narrative)
            self._run_live(sc.inject.live)     # live first: if the stack refuses, the map does not pretend otherwise
            if self.state["mode"] == "live" and not self.state["dry_run"]:
                self.state["live_undo"] = _merge_undo(self.state.get("live_undo") or [], undo_actions(sc))
            self._baseline()
            self.state.update(scenario_id=sc.id, phase="injected", step_index=0, touched=[], pre_inject=self._capture())
            self.apply_effects(sc.inject.sim)
            sources = list(sc.inject.sim) or list(sc.focus)
            self.state["ripple"] = self._ripple(sources)
            self.apply_effects({i.node: _impact_effect(i) for i in sc.impact})
            if sc.steps:
                self.state["phase"] = "resolving"
        return self.snapshot()

    def run_step(self, index: int | None = None) -> dict:
        sid = self.state["scenario_id"]
        if not sid:
            raise ValueError("no active scenario")
        sc = self.scenarios[sid]
        idx = self.state["step_index"] if index is None else index
        if idx != self.state["step_index"]:
            raise ValueError(f"next step is {self.state['step_index'] + 1}, not {idx + 1}")
        if idx >= len(sc.steps):
            raise ValueError("scenario is already resolved")
        step = sc.steps[idx]
        with self._operation(f"step {idx + 1}/{len(sc.steps)}: {step.title}"):
            self.log(f"Step {idx + 1}/{len(sc.steps)}: {step.title}")
            self._run_live(step.live)          # live first: a failing action leaves the step pending, map unchanged
            self.apply_effects(step.sim)
            self.state["step_index"] = idx + 1
            if self.state["step_index"] >= len(sc.steps):
                self.state["phase"] = "resolved"
                self.log(f"{sc.id} resolved", "ok")
            if step.expect:
                self._check_expect(step)
        return self.snapshot()

    def _check_expect(self, step) -> None:
        node, want = step.expect.node, step.expect.health
        if self.state["mode"] == "live" and not self.state["dry_run"]:
            if self.probe_now:
                try:
                    self.probe_now()
                except Exception as e:  # noqa: BLE001
                    self.log(f"probe failed: {e}", "warn")
            # Live: only what a probe observed counts. The simulated health was just set by this very step, so
            # reading it back would always "pass".
            got = (self.state["observed"].get(node) or {}).get("health")
            if not got:
                self.log(f"expectation not verifiable live: no probe observes {node} (expected {want})", "warn")
                return
        else:
            got = self.effective_health(node)
        if got == want:
            self.log(f"✓ expectation: {node} is {got}", "ok")
        else:
            self.log(f"expectation not (yet) met: {node} is {got}, expected {want}", "warn")

    def clear_scenario(self) -> dict:
        with self._operation("clear scenario"):
            sid = self.state["scenario_id"]
            self._undo_live_faults()            # live first: if SingleStore refuses, the scenario stays as it is
            self._restore_scenario_map()
            self.log(f"Scenario {sid or ''} cleared: the nodes it touched are back to how they were before the injection. "
                     "Data that already landed and GitLab branches/tags stay - Reset for a clean stack.")
        return self.snapshot()

    def reload_content(self, world: World, scenarios: dict[str, Scenario]) -> None:
        """Swap in re-read YAML (main.maybe_reload). Node state is kept by id; new nodes start fresh, removed ones go;
        an active scenario that no longer exists is dropped."""
        with self._operation("reload scenario YAML"):
            with self._mut:
                self.world, self.scenarios = world, scenarios
                self.node_defs = {n.id: n for n in world.nodes}
                nodes = self._fresh_state(self.state["mode"])["nodes"]
                for nid, d in nodes.items():
                    old = self.state["nodes"].get(nid)
                    if old:
                        d.update(health=old["health"], note=old["note"], metrics={**d["metrics"], **old["metrics"]},
                                 knobs={**d["knobs"], **{k: v for k, v in old["knobs"].items() if k in d["knobs"]}})
                self.state["nodes"] = nodes
                self.state["touched"] = [t for t in self.state["touched"] if t in nodes]
                sid = self.state["scenario_id"]
                if sid and sid not in scenarios:
                    self.state.update(scenario_id=None, phase="idle", step_index=0, ripple={}, touched=[], pre_inject={})
                elif sid:
                    self.state["step_index"] = min(self.state["step_index"], len(scenarios[sid].steps))
                self.state["sprint_day"] = min(self.state["sprint_day"], len(world.sprint.days))
                self.content_version += 1
            self.log(f"Scenario YAML reloaded: {len(scenarios)} scenarios, {len(world.nodes)} nodes")

    def set_observed(self, observed: dict[str, dict], probes: dict[str, Any]) -> None:
        with self._mut:
            self.state["observed"] = observed
            self.state["probes"] = probes
        self.publish()

    # --------------------------------------------------------------- catalogue
    def catalogue(self) -> list[dict]:
        return [{"id": s.id, "layer": s.layer, "title": s.title, "summary": s.summary, "star": s.star,
                 "status": s.status, "focus": s.focus, "detect": s.detect, "stepCount": len(s.steps),
                 "live": bool(s.live_actions()), "requires": s.requirement_hints()} for s in self.scenarios.values()]

    def scenario_detail(self, sid: str) -> dict:
        sc = self.scenarios[sid]
        ctx = Ctx(settings=self.settings, dry_run=True, log=lambda _l: None, cache=self.cache)

        def rendered(acts: list[LiveAction]) -> list[str]:
            out: list[str] = []
            for a in acts:
                try:
                    out += actions.render(a.action, a.params(), ctx)
                except ActionError as e:
                    out.append(f"# {a.action}: {e}")
            return out

        return {**sc.model_dump(mode="json", exclude={"steps"}),
                "injectRendered": rendered(sc.requires) + rendered(sc.inject.live),
                "steps": [{**s.model_dump(mode="json", exclude={"live"}), "rendered": rendered(s.live),
                           "hasLive": bool(s.live)} for s in sc.steps]}


def _impact_effect(i):
    from .schema import Effect
    return Effect(health=i.health, note=i.note or "")
