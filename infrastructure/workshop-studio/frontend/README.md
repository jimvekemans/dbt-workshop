# Workshop Studio: frontend

Presenter-driven web app for a projector: a home page with twenty short scenarios in four groups, and inside each
scenario four parts (What goes wrong, The idea, The code, Live demo). React 19 + TypeScript (strict) + Vite. The only
runtime dependencies besides React are `highlight.js` (core, only sql/yaml/python) and two self-hosted variable fonts
(`@fontsource-variable/bricolage-grotesque`, `@fontsource-variable/jetbrains-mono`).

## Run it without a backend (the mock)

```
npm install
npm run dev          # then open http://127.0.0.1:5173/?mock
```

`?mock` switches on a complete in-browser fake of the studio API (`src/dev/mock.ts`, content in
`src/dev/mockContent.ts`): 16 of the 20 real scenarios, copied from the real API (`src/dev/fixtures/catalog.json`, `scenarios.json`) with the real recordings (`recordings.json`, converted from `../recordings/*.json`), every input and evidence type, a baseline that is "preparing" for the first 5 s, demo preparation (~3 s), steps of
4-8 s, one failing step (`one-clear-mail`, step 3, first attempt), cancel, leftovers and "Clean up now", Live and Replay.

| URL | Effect |
|---|---|
| `/?mock` | normal mock |
| `/?mock&fast=1` | all durations divided by 4 |
| `/?mock&offline=1` | the studio "cannot be reached": connection-lost banner |
| `/?mock&waiting=1` | the baseline check is being waited for (`waiting` in the state) |

`window.__studioMock.setOffline(true|false)` toggles the connection from the console. The mock is only included in dev
builds or when building with `VITE_MOCK=1`; the normal production bundle contains none of it.

The fixtures lack `docs-match-version`, `only-what-changed`, `production-waits` and `same-toolbox`, so the mock does not
show those four. There is no script to refresh them: they are plain copies of `GET /api/catalog`, `GET /api/scenarios/<id>`
and the recordings (see the header of `src/dev/mockContent.ts`), made by hand.

The repo root has `.claude/launch.json` with the config `studio-frontend-mock`, which is how the app's preview tooling
starts the mock: `npm run dev` on `127.0.0.1:5199` (strict port), then open `http://127.0.0.1:5199/?mock`.

Without `?mock`, the dev server proxies `/api` to the studio backend at `http://127.0.0.1:8085` (override with the
`STUDIO_API` environment variable) and adds the `X-Studio: 1` header. `npm run dev:live` does the same on a fixed
`127.0.0.1:5198` (strict port), the address `e2e/README.md` uses for a working tree.

## Scripts

| script | what |
|---|---|
| `npm run dev` | Vite dev server |
| `npm run dev:live` | dev server on `127.0.0.1:5198` (strict port), for use against the running backend |
| `npm run build` | `tsc --noEmit` then `vite build` into `dist/` |
| `npm run preview` | `vite preview`: serves the built `dist/` (run `build` first) |
| `npm test` | vitest (jsdom, testing-library) |
| `npm run typecheck` | `tsc --noEmit` |

## Where things are

- `src/api/types.ts`: the contract with the backend (catalog, scenario, state, evidence data). `src/api/client.ts`: HTTP
  client (header `X-Studio: 1`), SSE with reconnect and a connection-lost flag.
- `src/styles/tokens.css`: all design tokens (colour roles for light and dark, spacing, type scale, radii, shadows,
  motion). `base.css`: reset, focus ring, syntax colours, reduced motion. Each component has its own `*.module.css`.
- `src/lib/`: hash router, state reducer and selectors (`store.ts`), glossary `[[term]]` parser, formatting, theme, text size (`textSize.ts`: the five sizes behind the two "A" buttons, kept in `localStorage`).
- `src/components/`: one small component per file. `src/screens/`: Home, ScenarioPage and the four parts.
- Layout rules worth knowing: the header is one fixed-height row (the running job is a fixed pill at the bottom centre, above the sticky Back / Next bar from 1024 px, via `--footer-reserve`); environments are shown as development, validation and production (`src/lib/env.ts`); the home page has one status row whose Details open by themselves when something needs a look (`src/lib/health.ts`).

Optional contract fields the UI renders when present and ignores when absent (the mock sends them all): a flow node `state: "fix"`; a step `usual_seconds` and a scenario `prepare_usual_seconds`; a day input `mark: {day, label}`; an input `help` as `{on, off}`; a show item `before_of: "<step id>"` and `recorded_at`; a step `outcome_kind: "not_quite"` and `status: "stopped"`; a bar chart `unit` (item or data) for the y axis; a table row `details` (shown only under "Technical details"); the state's `waiting`; a catalog `first_pick` (id) or scenario `first_pick` (otherwise the first scenario of the first group is the good first pick). Also optional: bars `bars: [{key,label,today}]` (oldest first, today last, drawn instead of `labels`); run_history `tables_built/tables_total/not_run/checks_total/checks_failed/tables_failed` (chips); pipeline `failures: [{kind,text,rows,details}]` and job state `manual`; tasks `count`, `state_text` and the fold rule (3+ quiet rows after the first failure); result `details` (Technical details only). `[[term]]` is rendered as the plain word everywhere (popover where possible, plain in links, aria labels and the tab title). Scenario order on the home page is exactly the API's order. Nothing assumes a fixed number of scenarios: card rows are balanced per group (3 -> 3, 4 -> 2+2, 5 -> 3+2, 6 -> 3+3, 7 -> 4+3, 8 -> 4+4).

Third-round optional fields (all may be absent): run_history `tables_not_run`, `tables_failed`, `tables_left_out`, `checks_passed`, `checks_failed`, `checks_warned`, `checks_not_run`, `retry` (the chip says "No problems" only without failure and warning, "1 warning" amber, otherwise "Problems"; the run a step caused is "This run", a retry "Retry"); tasks rows `not_needed`, `part`, `parts` (a skipped row in a run without failure is "Not needed" anyway; repeated "one part" rows are numbered); pipeline `rehearsed: {tables, checks, tables_total, seconds, names}` (one large line "1 of 28 tables rehearsed" with a proportion bar and a quiet line); a table column `unit` and a table item/data `footer`; any item `caption` (shown only once it has data; a mail's caption is its reading guide, one line per hint); result `reasons` as strings or as `{label, left, right, same}` (a small comparison table, stacked at 390 px). Number cards keep a trail of the values they showed (from the successive states). In replay the evidence says "Recorded <date>", Stop is hidden and one line says what is fixed in the recording. The mock has a test scenario `states-test` (and `stress-test`) at the end of "releases" that shows all of this.

- Keyboard: `/` or Ctrl/Cmd+K opens the scenario switcher; left and right arrows move between the four parts (not while
  typing or using a slider); Esc closes popovers and dialogs.
