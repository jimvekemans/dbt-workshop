// Content for the in-browser mock. Everything here is a copy of what the real studio sends:
//   fixtures/catalog.json     GET /api/catalog
//   fixtures/scenarios.json   GET /api/scenarios/<id> for all 16 scenarios (without the do/setup/query keys)
//   fixtures/recordings.json  frames recorded from real runs (actions + evidence data per step)
// Steps that were recorded replay their real frames. The others get evidence generated in the real shapes.
import type {
  BarsData, Catalog, EvidenceData, EvidenceItem, FreshnessData, InputValues, MailData, NumberData, PipelineData, ProgressStatus, ResultData, RunHistoryData,
  RunState, Scenario, TableData, TasksData, VersionsData,
} from '../api/types'
import catalogJson from './fixtures/catalog.json'
import scenariosJson from './fixtures/scenarios.json'
import recordingsJson from './fixtures/recordings.json'
import { STRESS } from './mockStress'
import { STATES, statesEvidence } from './mockStates'

export const CATALOG = catalogJson as unknown as Catalog
export const SCENARIOS: Record<string, Scenario> = { ...(scenariosJson as unknown as Record<string, Scenario>), [STRESS.id]: STRESS, [STATES.id]: STATES }

export interface Frame {
  t: number
  actions: { label: string; status: ProgressStatus; summary?: string; details?: string }[]
  evidence: (EvidenceData | null)[]
}
export interface Recording {
  frames: Frame[]
  summary: string
  outcome: string
  outcome_kind?: 'good' | 'not_quite'
  recorded_at?: string | null
}
export const RECORDINGS = recordingsJson as unknown as Record<string, Record<string, Recording>>

/** The states scenario has no real recording: the mock "records" it once from its own generated frames. */
function recordStates() {
  RECORDINGS[STATES.id] = Object.fromEntries(
    STATES.demo.steps.map((def) => {
      const seconds = def.usual_seconds ?? 10
      const frames: Frame[] = Array.from({ length: 13 }, (_, k) => {
        const t = k / 12
        return { t: t * seconds, actions: [{ label: 'Running the step', status: t >= 1 ? ('done' as const) : ('running' as const), summary: t >= 1 ? 'Done.' : '' }], evidence: def.show.map((item) => (k === 0 ? null : fakeEvidence(item, { t, inputs: {}, attempt: 0 }))) }
      })
      return [def.id, { frames, summary: 'It finished.', outcome: def.outcome ?? '', recorded_at: '2026-11-15T17:15:00' } satisfies Recording]
    }),
  )
}

/** The real API already sends usual durations and the day mark; the mock only fills them in for its own stress scenario. */
function decorate() {
  const stress = SCENARIOS[STRESS.id]
  if (!stress) return
  stress.prepare_usual_seconds ??= 140
  stress.demo.steps.forEach((s) => {
    const frames = RECORDINGS[stress.id]?.[s.id]?.frames
    if (frames?.length) s.usual_seconds ??= Math.round(frames[frames.length - 1]!.t)
    else s.usual_seconds ??= null
  })
}

export function isRecorded(id: string): boolean {
  const sc = SCENARIOS[id]
  return !!sc && sc.demo.steps.every((s) => !!RECORDINGS[id]?.[s.id])
}

decorate()

export const TODAY = '2026-11-15'

export interface Ctx {
  t: number // 0..1 progress of the step
  inputs: InputValues
  attempt: number
}

const state = (t: number, at: number, to: number): RunState => (t >= to ? 'done' : t >= at ? 'running' : 'todo')
const WORD: Record<RunState, string> = { todo: 'Waiting to start', running: 'Running', done: 'Done', failed: 'Failed', skipped: 'Skipped' }

/** Evidence of every type in the shapes evidence.py returns, growing with t. `mock_rows` (stress test) sets a table's size. */
export function fakeEvidence(item: EvidenceItem, ctx: Ctx): EvidenceData {
  const t = ctx.t
  if (typeof item.mock === 'string') return statesEvidence(item, ctx)
  switch (item.type) {
    case 'table': {
      const columns = (item.columns as { key: string; label: string }[] | undefined) ?? [{ key: 'name', label: 'Name' }, { key: 'status', label: 'Result' }]
      const n = typeof item.mock_rows === 'number' ? item.mock_rows : 5
      const hl = item.highlight as { column: string; equals: string; as: 'good' | 'bad' } | undefined
      const rows = Array.from({ length: Math.round(n * Math.min(1, t * 1.2)) }, (_, r) => {
        const cells = Object.fromEntries(columns.map((c, ci) => [c.key, ci % 3 === 2 ? (r * 37 + ci * 11) % 1000 : `${c.label} ${r + 1}`]))
        const flagged = !!hl && r % 4 === 1
        if (flagged && hl) cells[hl.column] = hl.equals
        return { cells, state: flagged && hl ? hl.as : null, ...(flagged ? { details: `not_null_orders_order_id_${r}` } : {}) }
      })
      return { columns, rows, empty: 'Nothing to show yet.', truncated: n >= 200 } satisfies TableData
    }
    case 'bars': {
      const labels = Array.from({ length: 14 }, (_, i) => `2026-11-${String(2 + i).padStart(2, '0')}`)
      const good = (typeof item.good === 'string' ? item.good : 'readable')
      const bad = (typeof item.bad === 'string' ? item.bad : 'unreadable')
      const MONTH = 'Nov'
      const bars = labels.map((k, i) => ({ key: k, label: `${i + 2} ${MONTH}${i === labels.length - 1 ? ' (today)' : ''}`, ...(i === labels.length - 1 ? { today: true } : {}) }))
      return {
        labels,
        bars,
        unit: typeof item.unit === 'string' ? item.unit : 'parcel scans',
        x_label: String(item.x_label ?? ''),
        series: [
          { name: good, state: 'good', values: labels.map((_, i) => Math.round((90 + ((i * 17) % 30)) * Math.min(1, t * 1.5))) },
          { name: bad, state: 'bad', values: labels.map((_, i) => (i > 9 ? Math.round((40 + ((i * 7) % 20)) * Math.min(1, t * 1.5)) : 0)) },
        ],
      } satisfies BarsData
    }
    case 'number': {
      const cmp = item.compare_to as { label: string } | undefined
      return { value: Math.round(10_000 * Math.min(1, t * 1.3)), unit: String(item.unit ?? ''), compare: cmp ? { label: cmp.label, value: Math.round(10_000 * Math.min(1, t * 1.3) * 0.6) } : null } satisfies NumberData
    }
    case 'tasks': {
      const names = ['Checking for changes made outside the normal process', 'Building all the reports', 'Checking that every data feed is up to date', 'Sending the failure mail', 'Saving the run so it can be resumed']
      const tasks: TasksData['tasks'] = names.map((label, i) => {
        const s = state(t, i * 0.18, i * 0.18 + 0.2)
        return { label, state: s, state_text: WORD[s] }
      })
      const done = t >= 1
      if (done) {
        // the shape of a failed run: bookkeeping collapsed with a count, one failure, the rest skipped
        tasks.splice(0, tasks.length,
          { label: 'Preparing the run', state: 'done', state_text: 'Done in the first attempt', count: 5 },
          { label: 'Building all the reports', state: 'failed', state_text: 'Failed' },
          ...Array.from({ length: 15 }, (_, i) => ({ label: `Building report ${i + 1}`, state: 'skipped' as const, state_text: 'Skipped because an earlier step failed' })),
          { label: 'Sending the failure mail', state: 'done', state_text: 'Done' })
      }
      return {
        title: 'the nightly load',
        run: { state: done ? 'failed' : 'running', state_text: done ? 'Stopped by a problem' : 'Running', started: new Date(Date.now() - 60_000).toISOString(), finished: done ? new Date().toISOString() : null },
        tasks,
      } satisfies TasksData
    }
    case 'pipeline': {
      const jobs: PipelineData['jobs'] = ['Checking the code style', 'Building a private copy', 'Running every check', 'Releasing to production'].map((label, i) => {
        const s = state(t, i * 0.22, i * 0.22 + 0.25)
        return { label, state: s, state_text: s === 'done' ? 'Done' : s === 'running' ? 'Running' : 'Waiting' }
      })
      const done = t >= 1
      if (done) {
        jobs[2] = { label: 'Running every check', state: 'failed', state_text: 'Failed' }
        jobs[3] = { label: 'Releasing to production', state: 'manual', state_text: 'Waiting for a person' }
      }
      const failures: PipelineData['failures'] = done ? [{ kind: 'check', text: 'Every parcel must have a delivery address', rows: 12, details: 'not_null_parcels_address_id: got 12 results, configured to fail if != 0' }] : []
      return { state: done ? 'failed' : 'running', state_text: done ? 'Stopped by a problem' : 'Running', jobs, failures } satisfies PipelineData
    }
    case 'mail':
      return (t < 0.6
        ? { found: false, empty: 'No mail has arrived yet.' }
        : { found: true, subject: '[dev] dbt_daily_load failed: 1 error(s)', from: 'airflow-dev@workshop.local', received: new Date().toISOString(), html: '<h2 style="color:#b00020">dbt run failed</h2><p>Environment: <b>dev</b> &middot; 42 pass, 1 error</p>' }) satisfies MailData
    case 'versions': {
      const after = t >= 0.7
      return { environments: [{ env: 'dev', label: 'Development', tag: after ? 'v1.2.0' : 'v1.1.0' }, { env: 'val', label: 'Acceptance', tag: 'v1.1.0' }, { env: 'prd', label: 'Production', tag: 'v1.0.0' }] } satisfies VersionsData
    }
    case 'result':
      return (t < 1 ? { summary: '', data: {} } : { summary: 'SingleStore refused: svc_dev_dbt may not change tables in prd_edw_dv_bdv', data: {}, details: 'ERROR 1045 (28000): Access denied for user svc_dev_dbt on prd_edw_dv_bdv.orders' }) satisfies ResultData
    case 'freshness':
      return {
        generated_at: new Date().toISOString(),
        sources: [['carrier tracking events', 'late', 'Late', '3 hours ago'], ['olist customers', 'fresh', 'Up to date', '53 minutes ago'], ['olist orders', 'fresh', 'Up to date', '9 minutes ago'], ['web events', 'fresh', 'Up to date', 'just now']].map(([label, st, text, age]) => ({ label: label!, state: st as 'fresh' | 'late', state_text: text!, last_data: age! })),
      } satisfies FreshnessData
    case 'run_history': {
      const n = Math.min(typeof item.limit === 'number' ? item.limit : 5, 6)
      return { runs: Array.from({ length: n }, (_, i) => ({ started: new Date(Date.now() - i * 3_600_000).toISOString().slice(0, 19), state: i === 1 ? 'bad' : 'good', summary: i === 1 ? 'The night ended early: 1 check failed and the rest was left out.' : 'Everything built and every check passed.', ...(i === 1 ? { tables_built: 24, tables_total: 28, not_run: 6, checks_total: 21, checks_failed: 1, tables_failed: 0 } : { tables_built: 28, tables_total: 28, not_run: 0, checks_total: 21, checks_failed: 0, tables_failed: 0 }), ...(i === 0 ? { current: true } : {}) })), empty: 'No run has been recorded yet.' } satisfies RunHistoryData
    }
  }
}

/** Generic action list for steps without a recording: [label, summary, share of the time]. */
export function genericActions(title: string): [string, string, number][] {
  return [
    ['Changing the settings', 'Changed the settings in development.', 1],
    [`Running: ${title}`, 'It finished without problems in development.', 4],
    ['Checking the result', 'The result was read back.', 1],
  ]
}

recordStates()
