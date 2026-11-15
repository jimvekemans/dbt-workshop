#!/usr/bin/env python3
"""Run Workshop Studio scenarios end to end through the app's own API, the way the presenter would.

    run_scenario.py <id> [<id> ...] [--set <step>.<input>=<value> ...] [--cleanup] [--base URL]

For every scenario: "Getting ready", then every step with its default inputs (or the values given with --set), and
per step one line with the seconds it took, its status, the plain summary, the closing sentence and whether each
evidence item came back with data. A live run also (re)writes the scenario's recording.

Only the standard library is used, so it runs with the host's python3. The stack must be up (`make up`) and the
studio in live mode with no job running. Exit code 0 when every step ended as "done", 1 otherwise.
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request

HEADERS = {"Host": "localhost:8085", "X-Studio": "1", "Content-Type": "application/json"}


def call(base: str, method: str, path: str, body: dict | None = None):
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(base + path, method=method, headers=HEADERS, data=data)
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, json.loads(response.read() or b"null")
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode()[:300]


def wait_idle(base: str, limit: float) -> tuple[dict, float]:
    """Poll the state until no job runs; returns the state and the seconds waited."""
    started = time.time()
    while time.time() - started < limit:
        _, state = call(base, "GET", "/api/state")
        if isinstance(state, dict) and not state.get("job"):
            return state, time.time() - started
        time.sleep(0.5)
    raise SystemExit(f"Gave up after {int(limit)} seconds: a job is still running (see GET /api/state).")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("scenarios", nargs="+", help="scenario ids, run in this order")
    parser.add_argument("--set", action="append", default=[], metavar="STEP.INPUT=VALUE",
                        help="use this value instead of the input's default (repeatable)")
    parser.add_argument("--cleanup", action="store_true", help="put everything back after the last scenario")
    parser.add_argument("--base", default="http://127.0.0.1:8085", help="where the studio listens")
    parser.add_argument("--limit", type=float, default=1200, help="seconds to wait for one job")
    args = parser.parse_args()

    chosen: dict[tuple[str, str], str] = {}
    for item in args.set:
        key, _, value = item.partition("=")
        step, _, name = key.partition(".")
        chosen[(step, name)] = value

    _, state = call(args.base, "GET", "/api/state")
    if not isinstance(state, dict):
        raise SystemExit(f"The studio does not answer at {args.base}: is the stack up?")
    if state.get("mode") != "live":
        raise SystemExit("The studio is in replay mode: switch it to live first (home page, or POST /api/mode).")
    wait_idle(args.base, args.limit)

    failed = False
    for scenario_id in args.scenarios:
        code, scenario = call(args.base, "GET", f"/api/scenarios/{scenario_id}")
        if code >= 300:
            print(f"{scenario_id}: not found ({code}) {scenario}")
            failed = True
            continue
        code, out = call(args.base, "POST", f"/api/scenarios/{scenario_id}/prepare")
        if code >= 300:
            print(f"{scenario_id}: getting ready was refused ({code}) {out}")
            failed = True
            continue
        state, seconds = wait_idle(args.base, args.limit)
        demo = state.get("demo") or {}
        print(f"{scenario_id}: ready in {seconds:.1f}s, status {demo.get('status')}, {demo.get('message') or ''}".rstrip(", "))
        if demo.get("status") != "ready":
            failed = True
            continue
        for step in scenario["demo"]["steps"]:
            inputs = {i["id"]: chosen.get((step["id"], i["id"]), i.get("default")) for i in step.get("inputs", [])}
            code, out = call(args.base, "POST", f"/api/scenarios/{scenario_id}/steps/{step['id']}/run", {"inputs": inputs})
            if code >= 300:
                print(f"   {step['id']}: refused ({code}) {out}")
                failed = True
                break
            state, seconds = wait_idle(args.base, args.limit)
            now = next((s for s in (state.get("demo") or {}).get("steps", []) if s.get("id") == step["id"]), {})
            evidence = ", ".join(f"{e.get('type')}:{'ok' if e.get('data') else 'EMPTY'}" for e in now.get("evidence", []))
            print(f"   {step['id']}: {seconds:.1f}s {now.get('status')} ({now.get('outcome_kind') or '-'})")
            print(f"      summary: {now.get('summary')}")
            print(f"      outcome: {now.get('outcome')}")
            print(f"      evidence: {evidence}")
            if now.get("status") != "done":
                failed = True
                break

    if args.cleanup:
        call(args.base, "POST", "/api/cleanup")
        time.sleep(1)
        state, seconds = wait_idle(args.base, args.limit)
        print(f"clean-up: {seconds:.1f}s, left behind: {state.get('leftovers') or 'nothing'}")
        failed = failed or bool(state.get("leftovers"))
    else:
        _, state = call(args.base, "GET", "/api/state")
        left = [item.get("label") for item in state.get("leftovers") or []]
        print(f"left behind (put back when the next scenario starts): {left or 'nothing'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
