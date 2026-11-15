"""Forget the studio's saved state and let the running app check the practice system again.

    docker exec workshop-studio python -m studio.reset

Use it after `make reset`, or whenever the studio keeps talking about something that is no longer there. It forgets what
demos left behind, the data feeder's state and the saved code baseline, then the app checks (and where needed rebuilds)
data, versions and tables of every environment. It refuses while a job is running. When the app is not running, the
saved files are removed directly and the next start checks everything.
"""
from __future__ import annotations

import json
import os
import sys
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
            return 202, "The saved state is forgotten. The app is checking the practice system again; this takes " \
                        "about a minute when something has to be rebuilt. Progress shows on the home page."
    except urllib.error.HTTPError as e:
        try:
            detail = json.loads(e.read().decode()).get("detail", "")
        except ValueError:
            detail = ""
        return e.code, str(detail) or f"The app refused the request ({e.code})."
    except (urllib.error.URLError, OSError):
        return 0, ""


def main(settings: Settings | None = None, port: int | None = None) -> int:
    settings = settings or Settings.from_env()
    status, sentence = ask_app(port or int(os.environ.get("STUDIO_PORT", "8085")))
    if status == 0:
        for name in SAVED:
            (settings.data_dir / name).unlink(missing_ok=True)
        sentence = "The app is not running. The saved state is removed; the next start checks the practice system."
        status = 202
    print(sentence)
    return 0 if status == 202 else 1


if __name__ == "__main__":
    sys.exit(main())
