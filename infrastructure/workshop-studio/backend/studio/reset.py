"""Forget the studio's saved state and let the running app check the practice system again.

    docker exec workshop-studio python -m studio.reset [--wait SECONDS]

Use it after `make reset`, or whenever the studio keeps talking about something that is no longer there. It forgets what
demos left behind, the data feeder's state and the saved code baseline, then the app checks (and where needed rebuilds)
data, versions and tables of every environment. It refuses while a job is running. When the app is not running, the
saved files are removed directly and the next start checks everything. A running check of the baseline (the one a cold
start begins before bootstrap has built anything) is stopped and replaced; a running demo is not. With `--wait` the
command then waits until the check has finished and exits 0 only when the baseline is ready (bootstrap uses it).
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.error
import urllib.request

from .config import Settings

SAVED = ("state.json", "sql_undo.json", "feeder.json", "git_baseline.json")


def ask_app(port: int) -> tuple[int, str]:
    """POST to the local API with the guard header; returns (status, plain sentence). Status 0: the app is not there."""
    request = urllib.request.Request(f"http://127.0.0.1:{port}/api/reset", method="POST", data=b"",
                                     headers={"X-Studio": "1", "Host": f"localhost:{port}"})
    try:
        with urllib.request.urlopen(request, timeout=20):
            return 202, "De bewaarde toestand is gewist. De app kijkt het oefensysteem opnieuw na; dat duurt ongeveer " \
                        "een minuut als er iets opnieuw gebuild moet worden. De voortgang zie je op de startpagina."
    except urllib.error.HTTPError as e:
        try:
            detail = json.loads(e.read().decode()).get("detail", "")
        except ValueError:
            detail = ""
        return e.code, str(detail) or f"De app weigerde de aanvraag ({e.code})."
    except (urllib.error.URLError, OSError):
        return 0, ""


def read_baseline(port: int) -> tuple[str, str, bool]:
    """(status, detail, busy) of the baseline from GET /api/state; status "unknown" when the app does not answer."""
    request = urllib.request.Request(f"http://127.0.0.1:{port}/api/state", headers={"Host": f"localhost:{port}"})
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            state = json.loads(response.read().decode())
        baseline = state.get("baseline") or {}
        return str(baseline.get("status", "unknown")), str(baseline.get("detail", "")), bool(state.get("job"))
    except (urllib.error.URLError, OSError, ValueError):
        return "unknown", "De app antwoordt niet.", False


def wait_until_ready(port: int, timeout: float, interval: float = 5.0) -> int:
    """Poll until the baseline check has finished: 0 when it is ready, 1 when it failed or `timeout` seconds passed.
    A failed status only counts once no job runs: the check tries a few times and shows "failed" in between."""
    deadline, last = time.monotonic() + timeout, ""
    while True:
        status, detail, busy = read_baseline(port)
        if status == "ready" and not busy:
            print("Het oefensysteem is klaar: elke omgeving heeft data, een versie en gebuilde tabellen.")
            return 0
        if status == "failed" and not busy:
            print(f"Het oefensysteem nakijken is mislukt: {detail}")
            return 1
        if detail != last:
            last = detail
            print(f"  ... {detail or status}")
        if time.monotonic() >= deadline:
            print(f"Het oefensysteem was niet klaar binnen {int(timeout)} seconden ({detail or status}).")
            return 1
        time.sleep(interval)


def main(settings: Settings | None = None, port: int | None = None, wait: float | None = None) -> int:
    settings = settings or Settings.from_env()
    port = port or int(os.environ.get("STUDIO_PORT", "8085"))
    status, sentence = ask_app(port)
    app_running = status != 0
    if status == 0:
        for name in SAVED:
            (settings.data_dir / name).unlink(missing_ok=True)
        sentence = "De app draait niet. De bewaarde toestand is verwijderd; bij de volgende start wordt het oefensysteem nagekeken."
        status = 202
    print(sentence)
    if status != 202:
        return 1
    return wait_until_ready(port, wait) if wait and app_running else 0


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Forget the studio's saved state and check the baseline again.")
    parser.add_argument("--wait", type=float, metavar="SECONDS", default=None,
                        help="wait (at most this long) until the baseline is ready; exit 1 if it is not")
    sys.exit(main(wait=parser.parse_args().wait))
