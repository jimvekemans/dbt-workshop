// A scenario that is not on the real home page: every state the interface has, in one place (open #/s/states-test/problem in
// the mock, in Live and in Replay). A failed night, a night with a warning, a retry, collapsed "not run" groups, "Not needed" rows,
// numbered parts, a rehearsal line, a table with a footer, a long mail, before/after pairs, a locked step, fixed inputs in replay,
// a number card with a trail. Evidence items carry `mock: '<kind>'`; mockContent.fakeEvidence hands them to `statesEvidence`.
import type { CodeBlockData, EvidenceData, EvidenceItem, PipelineData, RunHistoryData, Scenario, TableData, TasksData } from '../api/types'

const lines = (n: number): CodeBlockData['lines'] =>
  Array.from({ length: n }, (_, i) => ({
    n: 120 + i,
    text:
      i === 2 ? "select order_id, json_extract_string(payload, 'carrier_code') as carrier_code, _ingested_at, 'a long literal that is wider than the narrow screens of phones and tablets' as note from {{ ref('stg_orders') }}"
      : i % 5 === 0 ? '-- the carrier sends one scan per parcel and per hour, read with the format of the day it arrived'
      : `select column_${i}, 'x' as marker_${i} from {{ ref('model_${i}') }}`,
    ...([2, 9].includes(i) ? { note: i === 2 ? 'The format is chosen by the day the scan ARRIVED, so older scans keep their old format.' : 'A second note under its own line, with the same number badge as the marker.' } : {}),
  }))

export const STATES: Scenario = {
  id: 'states-test',
  group: 'releases',
  title: 'Every state of the screen, in one place',
  promise: 'A test scenario for the studio itself: a failed night, a warning, a retry, a long mail and a before and after.',
  minutes: 6,
  environment: 'dev',
  related: [{ id: 'switchover-date', why: 'The same day slider, with the real recording.' }],
  problem: {
    headline: 'The night ends early and nobody can see why',
    story: 'The nightly load builds 28 tables and runs 21 checks. One check fails, so everything after it is skipped.\n\nThe team finds out at nine in the morning, when the reports are empty.',
    cost: 'Every report that depends on the failed table is wrong or empty until somebody notices.',
    visual: {
      type: 'timeline',
      days: [
        { label: '7 days ago', state: 'ok', note: 'About 110 scans read' },
        { label: '6 days ago', state: 'ok', note: 'About 110 scans read' },
        { label: '5 days ago', state: 'ok', note: 'About 110 scans read' },
        { label: '4 days ago', state: 'bad', note: '0 of about 110 scans read' },
        { label: '3 days ago', state: 'bad', note: '0 of about 110 scans read' },
        { label: 'Yesterday', state: 'bad', note: '0 of about 110 scans read' },
        { label: 'Today', state: 'bad', note: '0 scans read so far' },
      ],
      marker: { after: 2, label: 'The carrier changes its format' },
      caption: 'From the day of the change, every scan arrives and none can be read.',
    },
  },
  idea: {
    headline: 'Two readers, one report: pick by the day the scan arrived',
    points: ['Every scan is read with the format of the day it arrived.', 'The old reader keeps working for old scans.', 'The new reader takes over from the switch day.'],
    visual: {
      type: 'flow',
      nodes: [
        { id: 'scans', label: 'Scans arrive', state: 'ok' },
        { id: 'day', label: 'The switch day decides', state: 'fix', note: 'One setting, no release.' },
        { id: 'old', label: 'Old reader', state: 'ok', note: 'Scans before the switch day' },
        { id: 'new', label: 'New reader', state: 'ok', note: 'Scans from the switch day' },
        { id: 'report', label: 'The report', state: 'ok' },
      ],
      links: [['scans', 'day'], ['day', 'old'], ['day', 'new'], ['old', 'report'], ['new', 'report']],
      caption: 'Either reader, never both; the report always gets readable scans.',
    },
  },
  code: [
    { title: 'Choosing the reader by day', file_label: 'The recipe for the table of parcel scans', file: 'models/edw_dv_bdv/carrier/stg_carrier_events.sql', language: 'sql', gitlab_url: 'http://gitlab.local/workshop/dbt-vdp/-/blob/dev/models/x.sql#L120-L140', lines: lines(22) },
    { title: 'A second file', file_label: 'The settings of the nightly load', file: 'airflow/dags/dbt_daily_load.py', language: 'python', gitlab_url: 'http://gitlab.local/workshop/dbt-vdp/-/blob/dev/airflow/x.py#L10-L20', lines: lines(8).map((l) => ({ ...l, text: l.text.slice(0, 60) })) },
  ],
  prepare_usual_seconds: 25,
  demo: {
    intro: 'A night fails, you look at what it did, then a retry finishes the rest. Shown in validation, our checking copy.',
    steps: [
      {
        id: 'night', title: 'Run the night and watch it stop', say: 'The load runs and one check fails. Look at the list of steps: what ran, what was skipped and what never started.', button: 'Run the night', usual_seconds: 15,
        inputs: [
          { id: 'day', type: 'day', label: 'Which day does the carrier switch?', help: 'The marked day is when the carrier changed.', mark: { day: '2026-11-11', label: '11 Nov: the carrier really changed its format' }, default: '2026-11-11', min: '2026-11-08', max: '2026-11-15', step: null, unit: null, options: null },
          { id: 'team', type: 'choice', label: 'Which team is not ready?', help: null, default: 'reviews', min: null, max: null, step: null, unit: null, options: [{ value: 'payments', label: 'The payments team' }, { value: 'reviews', label: 'The reviews team' }] },
          { id: 'parts', type: 'number', label: 'Cut the rebuild into how many parts?', help: 'A part is a slice of the rows that is loaded on its own.', default: 4, min: 2, max: 8, step: 1, unit: 'parts', options: null },
        ],
        show: [
          { type: 'number', title: 'Rows readers can see', mock: 'trail', unit: 'rows', compare_to: { label: 'Rows in the new copy' } },
          { type: 'tasks', title: 'The nightly load, step by step', mock: 'failed-night' },
          { type: 'pipeline', title: 'The automatic rehearsal', mock: 'rehearsal' },
          { type: 'table', title: 'The slowest steps and checks', mock: 'slowest', footer: 'Showing the 9 slowest of 28 tables and 21 checks.' },
          { type: 'run_history', title: 'The latest nights', mock: 'history', limit: 6 },
          { type: 'versions', title: 'Version in each environment' },
          { type: 'bars', title: 'Parcel scans per day', x_label: 'Day', unit: 'parcel scans', good: 'readable', bad: 'unreadable' },
        ],
        outcome: 'The night stopped at the failed check: three steps never started and ten were skipped. The failure mail still went out.',
      },
      {
        id: 'after', title: 'Retry and read the mail', say: 'The retry builds only what is missing. The mail the person on duty gets is shown full width.', button: 'Retry the night', usual_seconds: 10,
        inputs: [],
        show: [
          { type: 'mail', title: 'What the person on duty receives', mock: 'mail', caption: 'The subject says what failed.\nThe failed check is named near the top.\nThe log lines for the engineer come last.' },
          { type: 'pipeline', title: 'The automatic rehearsal', mock: 'rehearsal-4', before_of: 'night' },
          { type: 'versions', title: 'Version in each environment', before_of: 'night' },
          { type: 'number', title: 'Rows readers can see', mock: 'trail', unit: 'rows', before_of: 'night' },
          { type: 'bars', title: 'Parcel scans per day', x_label: 'Day', unit: 'parcel scans', good: 'readable', bad: 'unreadable', before_of: 'night' },
          { type: 'run_history', title: 'The latest nights, after the retry', mock: 'history-retry', limit: 6 },
        ],
        outcome: 'The retry built the 4 tables that had not run and 3 checks. Nothing had to be built twice.',
      },
      {
        id: 'clean', title: 'A night without problems', say: 'Nothing fails. Steps that are only there for a failure say "Not needed", and the repeated steps are numbered.', button: 'Run a clean night', usual_seconds: 8,
        inputs: [],
        show: [
          { type: 'tasks', title: 'The rebuild, step by step', mock: 'clean-run' },
          { type: 'table', title: 'Checks that failed', mock: 'empty-table' },
          { type: 'result', title: 'The two toolboxes', mock: 'compare' },
          { type: 'result', title: 'The same, as plain lines', mock: 'compare-plain' },
        ],
        outcome: 'Everything ran and the two failure-only steps were not needed.',
      },
    ],
  },
}


const failedNight = (t: number): TasksData => {
  const done = t >= 1
  const tasks: TasksData['tasks'] = done
    ? [
        { label: 'Preparing the run', state: 'done', state_text: 'Done the first time', count: 5 },
        { label: 'Checking for changes made outside the normal process', state: 'done', state_text: 'Done' },
        { label: 'Building all the reports', state: 'failed', state_text: 'Failed' },
        { label: 'Building report A', state: 'todo', state_text: 'Not run' },
        { label: 'Building report B', state: 'todo', state_text: 'Not run' },
        { label: 'Building report C', state: 'todo', state_text: 'Not run' },
        ...Array.from({ length: 10 }, (_, i) => ({ label: `Saving results ${i + 1}`, state: 'skipped' as const, state_text: 'Skipped because an earlier step failed' })),
        { label: 'Sending the failure mail', state: 'done', state_text: 'Done' },
        { label: 'Resuming the nightly load', state: 'done', state_text: 'Done' },
      ]
    : [
        { label: 'Preparing the run', state: t >= 0.2 ? 'done' : 'running', state_text: t >= 0.2 ? 'Done' : 'Running' },
        { label: 'Building all the reports', state: t >= 0.6 ? 'failed' : t >= 0.2 ? 'running' : 'todo', state_text: t >= 0.6 ? 'Failed' : t >= 0.2 ? 'Running' : 'Not started' },
      ]
  return { title: 'the nightly load', run: { state: done ? 'failed' : 'running', state_text: done ? 'Stopped by a problem' : 'Running', started: '2026-11-15T17:14:02', finished: null }, tasks }
}

const cleanRun = (t: number): TasksData => {
  const parts = [1, 2, 3, 4].map((n) => ({ label: 'Loading one part of the new copy', state: t >= 0.2 * (n + 1) ? ('done' as const) : t >= 0.2 * n ? ('running' as const) : ('todo' as const), state_text: t >= 0.2 * (n + 1) ? 'Done' : t >= 0.2 * n ? 'Running' : 'Not started' }))
  const end = t >= 1
  return {
    title: 'the rebuild',
    run: { state: end ? 'done' : 'running', state_text: end ? 'Finished' : 'Running', started: '2026-11-15T17:16:40', finished: null },
    tasks: [
      { label: 'Preparing the new copy', state: 'done', state_text: 'Done' },
      { label: 'Checking the old copy', state: 'done', state_text: 'Done' },
      ...parts,
      { label: 'Putting the new copy in place', state: end ? 'done' : 'todo', state_text: end ? 'Done' : 'Not started' },
      { label: 'Removing the unfinished copy', state: end ? 'skipped' : 'todo', state_text: end ? 'Skipped' : 'Not started', ...(end ? { not_needed: true } : {}) },
      { label: 'Sending the failure mail', state: end ? 'skipped' : 'todo', state_text: end ? 'Skipped' : 'Not started', ...(end ? { not_needed: true } : {}) },
    ],
  }
}

const run = (i: number, over: Partial<RunHistoryData['runs'][number]>): RunHistoryData['runs'][number] => ({ started: `2026-11-15T17:${String(16 - i).padStart(2, '0')}:${String(12 + i * 7).padStart(2, '0')}`, state: 'good', summary: '', ...over })

export function statesEvidence(item: EvidenceItem, ctx: { t: number }): EvidenceData {
  const t = ctx.t
  switch (item.mock) {
    case 'trail': {
      const v = t < 0.15 ? 0 : t < 0.35 ? 25_000 : t < 0.55 ? 50_000 : t < 0.75 ? 75_000 : t < 1 ? 100_000 : 0
      return { value: v, unit: 'rows', compare: { label: 'Rows in the new copy', value: 100_000 } }
    }
    case 'failed-night':
      return failedNight(t)
    case 'clean-run':
      return cleanRun(t)
    case 'rehearsal':
    case 'rehearsal-4': {
      const done = t >= 1
      const jobs: PipelineData['jobs'] = [
        { label: 'Trying the change in a private copy', state: t >= 0.3 ? 'done' : 'running', state_text: t >= 0.3 ? 'Done' : 'Running' },
        { label: 'Running the checks', state: done ? 'failed' : t >= 0.3 ? 'running' : 'todo', state_text: done ? 'Failed' : t >= 0.3 ? 'Running' : 'Not started' },
        { label: 'Waiting for a person to accept', state: done ? 'manual' : 'todo', state_text: 'Waiting for a person' },
      ]
      return { state: done ? 'failed' : 'running', state_text: done ? 'Stopped by a problem' : 'Running', jobs, rehearsed: done ? (item.mock === 'rehearsal-4' ? { tables: 4, checks: 3, tables_total: 28, seconds: 9, names: ['the orders report', 'the customers report', 'and 2 more'] } : { tables: 1, checks: 0, tables_total: 28, seconds: 2, names: ['the management report'] }) : null, failures: done ? [{ kind: 'check', text: 'Every parcel must have a delivery address', rows: 3, details: 'not_null_parcels_address_id: got 3 results, configured to fail if != 0' }] : [] }
    }
    case 'slowest': {
      const columns = [{ key: 'name', label: 'Name' }, { key: 'kind', label: 'Kind' }, { key: 'seconds', label: 'Time', unit: 'seconds' }, { key: 'rows', label: 'Rows that broke the rule' }]
      const names: [string, string, number, number | null, boolean][] = [
        ['Parcel scans', 'table', 5.1, null, false], ['Orders', 'table', 4.2, null, false], ['Customers', 'table', 2.9, null, false],
        ['Check: each order number appears once', 'check', 1.8, 0, false], ['Check: each dummy reload key is unique', 'check', 0.6, 0, false],
        ['Check: the order number is never empty', 'check', 0.4, 3, true], ['dbt_utils_unique_combination_of_columns_dummy_reload_hkey_dummy_reload_load_date', 'check', 0.3, 0, false],
      ]
      const n = Math.max(0, Math.round(names.length * Math.min(1, t * 1.3)))
      return { columns, rows: names.slice(0, n).map(([name, kind, seconds, rows, bad]) => ({ cells: { name, kind, seconds, rows }, state: bad ? ('bad' as const) : null, ...(bad ? { details: 'source_not_null_raw_olist_orders_order_id: got 3 results, configured to fail if != 0' } : {}) })), empty: 'Nothing to show yet.', truncated: false } satisfies TableData
    }
    case 'compare':
      return t < 1 ? { summary: '', data: {} } : { summary: 'The two toolboxes differ in one tool.', details: 'raw: toolbox diff', data: { reasons: [{ label: 'dbt', left: '1.9.0', right: '1.9.0', same: true }, { label: 'sqlfluff', left: '3.1.0', right: '3.2.0', same: false }, { label: 'pre-commit', left: '', right: null, same: false }] } }
    case 'compare-plain':
      return t < 1 ? { summary: '', data: {} } : { summary: 'Plain lines still work.', details: 'raw: two lists compared', data: { reasons: ['The first list has 12 tools.', 'The second list has 13 tools.'] } }
    case 'empty-table':
      return { columns: [{ key: 'name', label: 'Check' }, { key: 'rows', label: 'Rows that broke the rule' }], rows: [], empty: 'No check failed in this night.', truncated: false }
    case 'history':
    case 'history-retry': {
      const retry = item.mock === 'history-retry'
      const runs: RunHistoryData['runs'] = [
        retry
          ? run(0, { current: true, retry: true, state: 'good', summary: 'Retry: 4 tables built, 3 checks passed', tables_built: 4, tables_total: 4, tables_left_out: 24, checks_passed: 3 })
          : run(0, { current: true, state: 'bad', summary: '24 of 28 tables built, 4 tables and 2 checks not run, 1 check failed', tables_built: 24, tables_total: 28, tables_not_run: 4, tables_failed: 0, checks_failed: 1, checks_passed: 14, checks_not_run: 2 }),
        run(1, { state: 'warn', summary: '28 of 28 tables built, 20 checks passed, 1 warning', tables_built: 28, tables_total: 28, checks_passed: 20, checks_warned: 1 }),
        run(2, { summary: '25 of 28 tables built, 3 left out on purpose, 14 checks passed', tables_built: 25, tables_total: 25, tables_left_out: 3, checks_passed: 14 }),
        run(3, { summary: '28 of 28 tables built, 21 checks passed', tables_built: 28, tables_total: 28, checks_passed: 21 }),
        run(4, { state: 'bad', summary: '24 of 28 tables built, 4 tables and 2 checks not run, 1 check failed', tables_built: 24, tables_total: 28, tables_not_run: 4, checks_failed: 1, checks_passed: 14, checks_not_run: 2 }),
      ]
      if (retry) runs.splice(1, 0, run(0.5, { state: 'bad', started: '2026-11-15T17:15:20', summary: '24 of 28 tables built, 4 tables and 2 checks not run, 1 check failed', tables_built: 24, tables_total: 28, tables_not_run: 4, checks_failed: 1, checks_passed: 14, checks_not_run: 2 }))
      return { runs: t < 0.4 ? [] : runs, empty: 'Nothing has run yet.' }
    }
    case 'mail': {
      if (t < 0.5) return { found: false, empty: 'No mail has arrived yet.' }
      const log = Array.from({ length: 40 }, (_, i) => `<tr><td style="font-family:monospace;font-size:12px;color:#57544a;padding:1px 8px 1px 0">17:14:${String(10 + i).padStart(2, '0')}</td><td style="font-family:monospace;font-size:12px">log line ${i + 1}: ran model ${i + 1} of 28 in ${(i * 0.37).toFixed(1)} s</td></tr>`).join('')
      return {
        found: true,
        subject: '[validatie] Nachtelijke load mislukt: 1 check mislukt, 4 stappen niet gerund',
        from: 'airflow-val@workshop.local',
        received: '2026-11-15T17:15:03',
        html: `<h2 style="color:#b01f17;margin:0 0 8px">Nachtelijke load mislukt</h2><p><b>Wat mislukte:</b> 3 rijen in raw_olist_orders hebben geen order_id</p><p>Omgeving: <b>validatie</b> &middot; 42 gelukt, 1 mislukt, 4 niet gerund</p><h3>Waar je moet kijken</h3><p>De mislukte check is source_not_null_raw_olist_orders_order_id. De laatste 40 regels uit de log staan eronder.</p><table>${log}</table>`,
      }
    }
    default:
      return { value: 0, unit: '', compare: null }
  }
}
