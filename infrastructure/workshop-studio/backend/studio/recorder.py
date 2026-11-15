"""Recording of live steps and their replay.

A recording is what the audience saw: per step a list of frames (the step's action list and the evidence data at a
moment, with seconds since the start). Replay feeds those frames through the same state updates, ten times faster.
"""
from __future__ import annotations

import copy
import json
import logging
import os
import statistics
import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Callable

from .actions.base import Cancelled

log = logging.getLogger("studio.recorder")
REPLAY_SPEED = 10.0
REPLAY_MIN_SECONDS = 1.5
KEEP_DURATIONS = 5        # how many durations of one step are kept; their median is "usual"


class FrameCollector:
    """Collects the frames of one live step."""

    def __init__(self, clock: Callable[[], float] = time.monotonic):
        self._clock, self._start = clock, clock()
        self.frames: list[dict[str, Any]] = []

    def frame(self, actions: list[dict[str, Any]], evidence: list[Any],
              items: list[tuple[str, str | None]] | None = None) -> None:
        """`items` are the (type, title) of the evidence items: each frame then stores `{type, title, data}` per
        item, so a recording can tell later whether it still fits the scenario."""
        data = copy.deepcopy(evidence)
        if items is not None:
            data = [{"type": t, "title": title, "data": d} for (t, title), d in zip(items, data)]
        self.frames.append({"t": round(self._clock() - self._start, 2), "actions": copy.deepcopy(actions),
                            "evidence": data})


def _is_item(entry: Any) -> bool:
    return isinstance(entry, dict) and set(entry) == {"type", "title", "data"}


def normalized(frames: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Frames whose evidence is always `[{type, title, data}]`. Recordings made before the types were stored hold
    bare data: their items get `type` and `title` None."""
    return [{**f, "evidence": [e if _is_item(e) else {"type": None, "title": None, "data": e}
                               for e in f.get("evidence", [])]} for f in frames]


class Recorder:
    """Reads and writes `recordings/<scenario id>.json`, keeping the recordings of the other steps."""

    def __init__(self, directory: Path):
        self.directory = directory
        self._lock = threading.Lock()

    def _path(self, scenario_id: str) -> Path:
        return self.directory / f"{scenario_id}.json"

    def _read(self, scenario_id: str) -> dict[str, Any]:
        try:
            return json.loads(self._path(scenario_id).read_text(encoding="utf-8"))
        except FileNotFoundError:
            return {"scenario": scenario_id, "steps": {}}

    def step_ids(self, scenario_id: str) -> set[str]:
        try:
            return set(self._read(scenario_id)["steps"])
        except (ValueError, KeyError, OSError):
            return set()

    def load_step(self, scenario_id: str, step_id: str) -> dict[str, Any] | None:
        """The recording of a step with normalized frames and its `recorded_at`, or None."""
        try:
            data = self._read(scenario_id)
            step = data["steps"].get(step_id)
        except (ValueError, KeyError, OSError, AttributeError):
            log.exception("recording of %s is unreadable", scenario_id)
            return None
        if step is None:
            return None
        return {**step, "frames": normalized(step.get("frames", [])),
                "outcome_kind": step.get("outcome_kind", "good"),
                "recorded_at": step.get("recorded_at") or data.get("recorded_at")}

    @staticmethod
    def types_of(step: dict[str, Any]) -> list[str | None] | None:
        """The evidence types a recorded step was made with, None when the recording cannot tell (no frames)."""
        if isinstance(step.get("show"), list):
            return step["show"]
        frames = step.get("frames") or []
        return [e["type"] for e in frames[-1]["evidence"]] if frames else None

    def matches(self, step: dict[str, Any], types: list[str]) -> bool:
        """Does the recording fit the step's current `show` list (types, in order)? An old recording without
        stored types is compared by the number of items only."""
        recorded = self.types_of(step)
        if recorded is None:
            return True
        if any(t is None for t in recorded):
            return len(recorded) == len(types)
        return recorded == types

    def load_matching(self, scenario_id: str, step_id: str, types: list[str]) -> dict[str, Any] | None:
        """Like `load_step`, but None when the step's `show` list no longer matches the recording."""
        step = self.load_step(scenario_id, step_id)
        return step if step is not None and self.matches(step, types) else None

    def current_steps(self, scenario_id: str, shows: dict[str, list[str]]) -> set[str]:
        """The step ids that have a recording fitting their current `show` types."""
        return {sid for sid, types in shows.items()
                if self.load_matching(scenario_id, sid, types) is not None}

    def usual_seconds(self, scenario_id: str, step_id: str) -> float | None:
        step = self.load_step(scenario_id, step_id)
        if step is None:
            return None
        durations = step.get("durations")
        if not durations and step.get("frames"):
            durations = [step["frames"][-1]["t"]]            # an old recording: the time of its last frame
        return _usual(durations)

    def prepare_usual_seconds(self, scenario_id: str) -> float | None:
        try:
            return _usual(self._read(scenario_id).get("prepare", {}).get("durations"))
        except (ValueError, KeyError, OSError, AttributeError):
            return None

    def _write(self, scenario_id: str, mutate: Callable[[dict[str, Any]], None]) -> None:
        with self._lock:
            try:
                data = self._read(scenario_id)
            except ValueError:
                log.warning("recording of %s was unreadable and is replaced", scenario_id)
                data = {"scenario": scenario_id, "steps": {}}
            mutate(data)
            self.directory.mkdir(parents=True, exist_ok=True)
            tmp = self._path(scenario_id).with_suffix(".json.tmp")
            tmp.write_text(json.dumps(data, indent=1, ensure_ascii=False), encoding="utf-8")
            os.replace(tmp, self._path(scenario_id))

    def save_step(self, scenario_id: str, step_id: str, inputs: dict[str, Any], frames: list[dict[str, Any]],
                  summary: str, outcome: str, outcome_kind: str = "good", duration: float | None = None,
                  show: list[str] | None = None) -> None:
        """Replace the recording of one step; write atomically so a crash never leaves a half file. The durations
        of earlier recordings of the step are kept (the last few), for the median shown as "usually"."""
        def mutate(data: dict[str, Any]) -> None:
            now = datetime.now(timezone.utc).isoformat(timespec="seconds")
            old = data["steps"].get(step_id) or {}
            durations = list(old.get("durations") or ([old["frames"][-1]["t"]] if old.get("frames") else []))
            if duration is not None:
                durations = (durations + [round(duration, 1)])[-KEEP_DURATIONS:]
            data["recorded_at"] = now
            data["steps"][step_id] = {"inputs": inputs, "frames": frames, "summary": summary, "outcome": outcome,
                                      "outcome_kind": outcome_kind, "recorded_at": now, "durations": durations,
                                      **({"show": show} if show is not None else {})}
        self._write(scenario_id, mutate)

    def save_prepare(self, scenario_id: str, duration: float) -> None:
        def mutate(data: dict[str, Any]) -> None:
            kept = list(data.get("prepare", {}).get("durations") or [])
            data["prepare"] = {"durations": (kept + [round(duration, 1)])[-KEEP_DURATIONS:]}
        self._write(scenario_id, mutate)


def _usual(durations: list[float] | None) -> float | None:
    """The median of the kept durations (the one value when there is one), None when there are none."""
    return round(float(statistics.median(durations)), 1) if durations else None


def replay_schedule(frames: list[dict[str, Any]], speed: float = REPLAY_SPEED,
                    min_seconds: float = REPLAY_MIN_SECONDS) -> list[tuple[float, dict[str, Any]]]:
    """(seconds after the start, frame) pairs: recorded times divided by `speed`, stretched so the whole step takes
    at least `min_seconds` (an instant step would otherwise flash by unseen)."""
    last = frames[-1]["t"] if frames else 0
    if last <= 0:
        return [(0.0 if i == 0 else min_seconds * i / max(len(frames) - 1, 1), f) for i, f in enumerate(frames)]
    factor = max(last / speed, min_seconds) / last
    return [(f["t"] * factor, f) for f in frames]


def replay(frames: list[dict[str, Any]], apply: Callable[[dict[str, Any]], None], cancel: threading.Event,
           speed: float = REPLAY_SPEED, min_seconds: float = REPLAY_MIN_SECONDS) -> None:
    """Apply each frame at its scheduled time; raises Cancelled when the presenter cancels."""
    start = time.monotonic()
    for at, frame in replay_schedule(frames, speed, min_seconds):
        if cancel.wait(max(0.0, start + at - time.monotonic())):
            raise Cancelled()
        apply(frame)
