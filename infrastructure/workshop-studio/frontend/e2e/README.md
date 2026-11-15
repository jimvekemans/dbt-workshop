# UI capture harness

Screenshots, control inventory, interaction log, console log, axe results and UI texts for every screen of the studio,
so reviewers without a browser can review it. Needs only Docker (official Playwright image; `playwright` and `axe-core`
are installed inside the container at run time, nothing on the host, nothing in package.json).

```
OUT=/path/to/review ./e2e/run.sh                              # the in-browser mock (dev server on :5199 must run)
BASE_URL=http://127.0.0.1:8085/ SCENARIOS=all OUT=... ./e2e/run.sh   # the real backend
```

Env: `BASE_URL` (default `http://127.0.0.1:5199/?mock`), `OUT` (default `./review`), `SCENARIOS` (comma list or `all`;
default: the two fully written mock scenarios), `WORKERS` (parallel pages in the interaction sweep, default 4),
`SKIP_SWEEP=1` (skip the slow per-control sweep), `PW_VERSION`.

Everything mock-specific (scenario ids, which step fails, replay ids) is in `CONFIG` at the top of `capture.mjs`.
The scenario list is taken from `GET /api/catalog` when reachable (the mock is in-browser, so it falls back to CONFIG).
Exit code: 0 ok, 1 fatal error, 2 a page crashed. Failed single captures are logged (`failures.json`) and skipped.

Outputs: `NN-<screen>-<state>.png`, `inventory.json`, `interactions.json`, `console.json`, `axe.json`, `texts.md`,
`assertions.json`, `failures.json`, `INDEX.md`. Accessible names are computed in-page (aria-labelledby, aria-label,
labels, text, title), not from Chromium's AX tree. The sweep reloads the page with `fast=1` (mock only).

## Against the real backend, unattended (replay mode)

```
BASE_URL=http://127.0.0.1:8085/ SCENARIOS=all OUT=/path/to/review ./e2e/run.sh      # add SKIP_SWEEP=1 for the quick run
BASE_URL=http://127.0.0.1:5198/ SCENARIOS=all OUT=...  ./e2e/run.sh                  # your working tree (npm run dev:live)
```

Without `?mock` the harness switches the backend to replay first (nothing in the stack changes), presses "Get the demo
ready" itself, blocks every page request that would switch to Live or touch the feeder (HTTP 409 from the harness), skips
the mock-only checks (feeder toggle, expected failing step, leftovers) and, at the end, puts mode and feeder back as they
were (`meta.backend_before` / `backend_after` in `INDEX.md`). A run of all sixteen scenarios with the sweep takes a long time; use
`SCENARIOS=a,b` and `SKIP_SWEEP=1` while iterating.

## Checks added in the third review round

Besides screenshots the run asserts: exactly one "Start here" badge and no orphan card (home), a Back / Next bar of at most 60 px with Back left of Next and every top-bar control on screen (desktop and phone), notes shown once and one GitLab link per code block, code lines wrapped on a phone, the end card's buttons never covered by the bar, the mail at card width with at least 15 px text and a full-size dialog that Esc closes, before/after pairs complete, and for the mock-only `states-test` scenario in Replay: fixed inputs in one line, no Stop, recorded times, a locked step, collapsed "not run" / "skipped" groups, "Not needed" rows, numbered parts and the table footer. The demo's button is "Prepare this demo".
