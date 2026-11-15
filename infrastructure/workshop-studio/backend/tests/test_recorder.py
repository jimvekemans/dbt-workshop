from __future__ import annotations

import json
import threading
import time

import pytest

from studio.actions.base import Cancelled
from studio.recorder import FrameCollector, Recorder, replay, replay_schedule


def frames(*times: float) -> list[dict]:
    return [{"t": t, "actions": [], "evidence": [t]} for t in times]


def test_round_trip_keeps_other_steps(tmp_path):
    rec = Recorder(tmp_path / "rec")
    assert rec.step_ids("s") == set() and rec.load_step("s", "a") is None
    rec.save_step("s", "a", {"x": 1}, frames(0, 2), "sum a", "out a")
    rec.save_step("s", "b", {}, frames(0), "sum b", "out b")
    rec.save_step("s", "a", {"x": 2}, frames(0, 4), "again", "out a2")
    assert rec.step_ids("s") == {"a", "b"}
    assert rec.load_step("s", "a")["inputs"] == {"x": 2} and rec.load_step("s", "b")["summary"] == "sum b"
    stored = json.loads((tmp_path / "rec" / "s.json").read_text())
    assert stored["scenario"] == "s" and "recorded_at" in stored
    assert not list((tmp_path / "rec").glob("*.tmp"))


def test_corrupt_file_is_replaced_on_save_and_ignored_on_read(tmp_path):
    rec = Recorder(tmp_path)
    (tmp_path / "s.json").write_text("{nope")
    assert rec.step_ids("s") == set() and rec.load_step("s", "a") is None
    rec.save_step("s", "a", {}, [], "x", "y")
    assert rec.step_ids("s") == {"a"}


def test_frame_collector_copies_and_stamps_time():
    clock = iter([100.0, 100.5, 102.0])
    collector = FrameCollector(lambda: next(clock))
    actions = [{"label": "a"}]
    collector.frame(actions, [1])
    actions[0]["label"] = "changed"
    collector.frame(actions, [2])
    assert [f["t"] for f in collector.frames] == [0.5, 2.0] and collector.frames[0]["actions"][0]["label"] == "a"


def test_schedule_divides_by_ten_with_a_minimum():
    assert [round(at, 2) for at, _ in replay_schedule(frames(0, 10, 20))] == [0, 1.0, 2.0]
    assert [round(at, 2) for at, _ in replay_schedule(frames(0, 1, 2))] == [0, 0.75, 1.5]       # stretched to 1.5 s
    assert [round(at, 2) for at, _ in replay_schedule(frames(0, 0, 0))] == [0, 0.75, 1.5]
    assert replay_schedule([]) == []


def test_replay_applies_frames_in_time_and_can_be_cancelled():
    seen: list[float] = []
    started = time.monotonic()
    replay(frames(0, 1), lambda f: seen.append(f["t"]), threading.Event(), speed=10, min_seconds=0.2)
    assert seen == [0, 1] and time.monotonic() - started >= 0.19
    cancel = threading.Event()
    cancel.set()
    with pytest.raises(Cancelled):
        replay(frames(0, 1), seen.append, cancel)
