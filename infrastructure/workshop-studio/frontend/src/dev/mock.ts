// In-browser fake of the studio API that emits the real payload shapes. Enabled with ?mock (dev builds) or VITE_MOCK=1.
//   ?mock                 normal run
//   ?mock&offline=1       the studio cannot be reached (connection-lost banner)
//   ?mock&fast=1          every duration divided by 4 (quick checks)
//   ?mock&live=off        Live is not available (the stack is unreachable), the server stays in Replay
//   ?mock&prepare=fail    getting a demo ready fails with a plain sentence
//   ?mock&notice=1        a notice from the last job is shown
// Recorded steps replay their real frames (fixtures/recordings.json); the rest get generated evidence.
import { ApiError } from '../api/types'
import type { ActionResult, Backend, Catalog, DemoStep, InputValues, LiveEvidence, Mode, ProgressStatus, Scenario, ServerState, StepDef, Subscription } from '../api/types'
import { CATALOG, RECORDINGS, SCENARIOS, fakeEvidence, genericActions, isRecorded, withIdeas } from './mockContent'
import { withFlowAndRoadmap } from './mockFlow'
import type { Frame } from './mockContent'

/** When the replayed frames were "recorded" (the mock invents a date; the real backend sends the real one). */
const RECORDED_AT = '2026-11-10T09:30:00'
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const BASELINE = ['dev', 'val', 'prd'].flatMap((e) => [`Demo data in ${e}`, `A numbered version running in ${e}`, `Tables built once in ${e}`])
const STACK: ServerState['stack'] = [
  { id: 'gitlab', label: 'Code repository', status: 'up', detail: 'Code storage is ready' },
  { id: 'singlestore', label: 'Database', status: 'up', detail: 'The database is ready' },
  { id: 'airflow-dev', label: 'Scheduler (development)', status: 'starting', detail: 'The scheduler is starting' },
  { id: 'airflow-val', label: 'Scheduler (validation)', status: 'up', detail: 'The scheduler is ready' },
  { id: 'airflow-prd', label: 'Scheduler (production)', status: 'up', detail: 'The scheduler is ready' },
  { id: 'mailpit', label: 'Mailbox', status: 'up', detail: 'The mailbox is ready' },
  { id: 'feeder', label: 'Data delivery', status: 'up', detail: 'Delivering data; today is 2026-11-15' },
]

export function createMockBackend(params: URLSearchParams): Backend {
  let offline = params.get('offline') === '1'
  const speed = params.get('fast') === '1' ? 4 : 1
  const liveOff = params.get('live') === 'off'
  const prepareFails = params.get('prepare') === 'fail'
  const listeners = new Set<Subscription>()
  const timers = new Set<ReturnType<typeof setTimeout> | ReturnType<typeof setInterval>>()
  const attempts: Record<string, number> = {}

  const S: ServerState = {
    mode: liveOff ? 'replay' : 'live',
    live_unavailable: liveOff ? 'The database cannot be reached, so only recordings can be shown.' : null,
    stack: structuredClone(STACK),
    baseline: liveOff
      ? { status: 'ready', detail: 'Replay mode: nothing runs on the stack.', progress: [] }
      : { status: 'preparing', detail: 'Checking the demo stack.', progress: BASELINE.map((label, i) => ({ label, status: i === 0 ? 'running' : 'todo' })) },
    feeder: { running: !liveOff, simulated_day: '2026-11-15', simulated_days: { dev: '2026-11-15', val: '2026-11-15', prd: '2026-11-15' }, last_delivery: new Date().toISOString().slice(0, 19) },
    job: null,
    leftovers: [],
    notice: params.get('notice') === '1' ? { level: 'info', text: 'Replay mode: nothing was changed, so there is nothing to clean up.' } : null,
    waiting: params.get('waiting') === '1' ? { label: 'Waiting', text: 'Waiting until the practice system is ready.' } : null,
    demo: null,
  }

  // ------------------------------------------------------------ plumbing
  const emit = () => {
    if (offline) return
    const snap = structuredClone(S)
    listeners.forEach((l) => l.onState(snap))
  }
  const later = (fn: () => void, ms: number) => {
    const t = setTimeout(() => {
      timers.delete(t)
      fn()
    }, ms / speed)
    timers.add(t)
    return t
  }
  const every = (fn: () => void, ms: number) => {
    const t = setInterval(fn, ms)
    timers.add(t)
    return t
  }
  const guard = async (): Promise<void> => {
    await sleep(120)
    if (offline) throw new ApiError('The studio cannot be reached right now.', 0)
  }
  const busy = () => {
    if (S.job) throw new ApiError(`Please wait: "${S.job.label}" is still running.`, 409)
  }
  const reply = () => structuredClone(S)
  const startJob = (kind: 'prepare' | 'step' | 'cleanup', scenario: string | null, step: string | null, label: string) => {
    S.job = { kind, scenario, step, label, started: Date.now() / 1000, cancellable: true, actions: [] }
    S.notice = null
  }

  // The first check of the stack: a job without a scenario, one baseline entry at a time.
  if (!liveOff) {
    startJob('prepare', null, null, 'Checking that the demo stack is ready')
    const stepMs = 5000 / BASELINE.length
    BASELINE.forEach((_, i) =>
      later(() => {
        if (S.job?.kind !== 'prepare' || S.job.scenario !== null) return
        S.baseline.progress = S.baseline.progress.map((p, j) => ({ ...p, status: (j <= i ? 'done' : j === i + 1 ? 'running' : 'todo') as ProgressStatus }))
        emit()
      }, stepMs * (i + 1) - 100),
    )
    later(baselineDone, 5000)
  }
  function baselineDone() {
    S.baseline = { status: 'ready', detail: 'Every environment has data, a version and built tables.', progress: BASELINE.map((label) => ({ label, status: 'done' as const })) }
    const dev = S.stack.find((s) => s.id === 'airflow-dev')
    if (dev) Object.assign(dev, { status: 'up', detail: 'The scheduler is ready' })
    if (S.job?.kind === 'prepare' && S.job.scenario === null) S.job = null
    emit()
  }
  every(() => {
    if (S.feeder.running && !S.job) {
      S.feeder.last_delivery = new Date().toISOString().slice(0, 19)
      emit()
    }
  }, 30_000)

  const emptyStep = (s: StepDef): Partial<DemoStep> => ({ status: 'todo', summary: '', outcome: '', outcome_kind: undefined, actions: [], evidence: s.show.map((e) => ({ ...e, data: null }) as LiveEvidence) })
  const todoSteps = (sc: Scenario): DemoStep[] => sc.demo.steps.map((s) => ({ id: s.id, status: 'todo', summary: '', outcome: '', actions: [], evidence: s.show.map((e) => ({ ...e, data: null }) as LiveEvidence) }))

  // ------------------------------------------------------------ prepare
  async function prepare(id: string): Promise<ServerState> {
    await guard()
    const sc = SCENARIOS[id]
    if (!sc) throw new ApiError('There is no such scenario.', 404)
    busy()
    startJob('prepare', id, null, `Getting ready: ${sc.title}`)
    S.demo = { scenario: id, status: 'preparing', message: '', steps: todoSteps(sc) }
    emit()
    const setup = (n: number, labels: string[]) => {
      if (S.job?.kind !== 'prepare') return
      S.job.actions = labels.map((label, i) => ({ label, status: (i < n ? 'done' : i === n ? 'running' : 'todo') as ProgressStatus, summary: i < n ? 'Done.' : '' }))
      emit()
    }
    const labels = ['Putting back what the last demo changed', 'Loading the practice data', 'Setting the scene']
    later(() => setup(0, labels), 300)
    later(() => setup(1, labels), 1300)
    later(() => setup(2, labels), 2300)
    later(() => {
      if (S.job?.kind !== 'prepare') return
      S.job = null
      if (prepareFails && S.demo) {
        S.demo.status = 'failed'
        S.demo.message = 'Could not get this ready: A numbered version running in val. The scheduler did not answer in time.'
        S.baseline = { ...S.baseline, status: 'failed', detail: S.demo.message }
      } else if (S.demo) S.demo.status = 'ready'
      emit()
    }, 3300)
    return reply()
  }

  // ------------------------------------------------------------ steps
  /** The frames a step plays: real recorded ones, or generated ones in the same shape. */
  function framesFor(id: string, def: StepDef, inputs: InputValues, attempt: number): { frames: Frame[]; seconds: number; summary: string; fail: string | null } {
    const rec = RECORDINGS[id]?.[def.id]
    if (rec) return { frames: rec.frames, seconds: Math.max(1, rec.frames[rec.frames.length - 1]?.t ?? 1), summary: rec.summary, fail: null }
    const acts = genericActions(def.title)
    const total = acts.reduce((a, x) => a + x[2], 0)
    const seconds = total * 2
    const fail = def.id === 'risky' && attempt === 0 ? 'The check found a problem, so the step stopped. Nothing more was changed.' : null
    const frames: Frame[] = Array.from({ length: 13 }, (_, k) => {
      const t = k / 12
      let acc = 0
      const actions: ActionResult[] = acts.map(([label, summary, w]) => {
        const start = acc / total
        acc += w
        const end = acc / total
        const status: ProgressStatus = t >= end ? 'done' : t >= start ? 'running' : 'todo'
        return { label, status: fail && t >= 1 && label === acts[2]![0] ? 'failed' : status, summary: t >= end ? summary : '', details: t >= end ? `mock: ${label}` : '' }
      })
      return { t: t * seconds, actions, evidence: def.show.map((item) => (k === 0 ? null : fakeEvidence(item, { t, inputs, attempt }))) }
    })
    return { frames, seconds, summary: 'It finished without problems.', fail }
  }

  async function runStep(id: string, stepId: string, inputs: InputValues): Promise<ServerState> {
    await guard()
    const sc = SCENARIOS[id]
    const def = sc?.demo.steps.find((s) => s.id === stepId)
    if (!sc || !def) throw new ApiError('There is no such step.', 404)
    busy()
    if (!S.demo || S.demo.scenario !== id || (S.demo.status !== 'ready' && S.demo.status !== 'running')) throw new ApiError('The demo is not ready. Press "Prepare this demo" first.', 409)
    const idx = sc.demo.steps.indexOf(def)
    const prev = sc.demo.steps[idx - 1]
    if (prev && S.demo.steps.find((s) => s.id === prev.id)?.status !== 'done') throw new ApiError(`Finish “${prev.title}” first.`, 409)
    const state = S.demo.steps.find((s) => s.id === stepId)!
    const key = `${id}/${stepId}`
    const attempt = attempts[key] ?? 0
    attempts[key] = attempt + 1

    if (S.mode === 'replay' && !RECORDINGS[id]?.[stepId]) {
      Object.assign(state, { status: 'failed', summary: 'This step has not been recorded yet.', actions: [] })
      emit()
      return reply()
    }
    const plan = framesFor(id, def, inputs, attempt)
    const play = S.mode === 'replay' ? Math.max(1.5, plan.seconds / 10) : Math.min(plan.seconds, 16)
    startJob('step', id, stepId, def.title)
    S.demo.status = 'running'
    S.demo.steps.slice(idx + 1).forEach((later_, k) => Object.assign(later_, emptyStep(sc.demo.steps[idx + 1 + k]!)))
    Object.assign(state, { status: 'running', summary: '', outcome: '', actions: [], evidence: def.show.map((e) => ({ ...e, data: null }) as LiveEvidence) })
    emit()

    plan.frames.forEach((f) =>
      later(() => {
        if (S.job?.kind !== 'step' || S.job.step !== stepId) return
        state.actions = structuredClone(f.actions)
        state.evidence = def.show.map((e, i) => ({ ...e, data: structuredClone(f.evidence[i] ?? null), ...(S.mode === 'replay' ? { recorded_at: RECORDINGS[id]?.[stepId]?.recorded_at ?? RECORDED_AT } : {}) }) as LiveEvidence)
        emit()
      }, (f.t / plan.seconds) * play * 1000),
    )
    later(() => {
      if (S.job?.kind !== 'step' || S.job.step !== stepId || !S.demo) return
      if (plan.fail) Object.assign(state, { status: 'failed', summary: plan.fail, outcome: '' })
      else {
        const rec = RECORDINGS[id]?.[stepId]
        // the mock knows one wrong answer: the carrier switched four days ago
        const day = typeof inputs.switch_day === 'string' ? inputs.switch_day : null
        const wrong = S.mode === 'live' && id === 'switchover-date' && stepId === 'with' && day !== null && day !== '2026-11-11'
        const outcome = wrong ? `Some days are still unreadable: you switched too ${day! > '2026-11-11' ? 'late' : 'early'}. Try another day.` : (rec?.outcome ?? def.outcome ?? '')
        Object.assign(state, { status: 'done', summary: plan.summary, outcome, outcome_kind: wrong ? 'not_quite' : (rec?.outcome_kind ?? 'good') })
        if (S.mode === 'live' && !S.leftovers.some((l) => l.from_scenario === id)) S.leftovers.push({ label: `The practice data changed by “${sc.title}”`, from_scenario: id })
      }
      S.demo.status = 'ready'
      S.job = null
      emit()
    }, play * 1000 + 300 * speed)
    return reply()
  }

  async function cleanup(): Promise<ServerState> {
    await guard()
    busy()
    startJob('cleanup', null, null, 'Putting everything back')
    emit()
    later(() => {
      S.leftovers = []
      S.demo = null
      S.job = null
      S.notice = S.mode === 'replay' ? { level: 'info', text: 'Replay mode: nothing was changed, so there is nothing to put back.' } : null
      emit()
    }, 2200)
    return reply()
  }

  async function cancel(): Promise<ServerState> {
    await guard()
    const j = S.job
    if (!j) return reply()
    timers.forEach((t) => {
      clearTimeout(t as ReturnType<typeof setTimeout>)
      clearInterval(t as ReturnType<typeof setInterval>)
    })
    timers.clear()
    every(() => {
      if (S.feeder.running && !S.job) {
        S.feeder.last_delivery = new Date().toISOString().slice(0, 19)
        emit()
      }
    }, 30_000)
    if (S.demo) {
      if (j.kind === 'step') {
        const sc = SCENARIOS[S.demo.scenario]
        const at = sc?.demo.steps.findIndex((s) => s.id === j.step) ?? -1
        // the stopped step is "Stopped"; every later step goes back to "Not yet" with no evidence
        if (sc && at >= 0) sc.demo.steps.slice(at).forEach((def, k) => { const st = S.demo?.steps.find((s) => s.id === def.id); if (st) Object.assign(st, emptyStep(def), k === 0 ? { status: 'stopped' } : {}) })
        S.demo.status = 'ready'
      } else if (j.kind === 'prepare') {
        S.demo.status = 'failed'
        S.demo.message = 'Getting ready was stopped. Press the start button to try again.'
      }
    }
    if (S.baseline.status === 'preparing') baselineDone()
    S.job = null
    emit()
    return reply()
  }

  // ------------------------------------------------------------ the Backend
  const api: Backend = {
    async getCatalog(): Promise<Catalog> {
      await sleep(250)
      const c = structuredClone(CATALOG)
      // the two test scenarios of the mock sit at the end of "releases" (7 cards there: a wide screen draws 4 + 3)
      const extra = [SCENARIOS['states-test'], SCENARIOS['stress-test']].filter((x): x is Scenario => !!x)
      const rel = c.groups.find((g) => g.id === 'releases')
      if (rel) extra.forEach((x) => rel.scenarios.push({ id: x.id, title: x.title, promise: x.promise, minutes: x.minutes, environment: x.environment, related: x.related, recorded: false }))
      c.groups.forEach((g) => g.scenarios.forEach((s) => (s.recorded = isRecorded(s.id))))
      // the sample flow, a few status labels and the closing overview (see mockFlow.ts)
      return withIdeas(withFlowAndRoadmap(c, isRecorded))
    },
    async getScenario(id) {
      await sleep(200)
      const s = SCENARIOS[id]
      if (!s) throw new ApiError('There is no such scenario.', 404)
      return structuredClone(s)
    },
    async getState() {
      await guard()
      return reply()
    },
    prepare,
    runStep,
    cleanup,
    cancel,
    async setMode(mode: Mode) {
      await guard()
      busy()
      if (mode === 'live' && S.live_unavailable) throw new ApiError(S.live_unavailable, 409)
      if (mode !== S.mode) {
        S.mode = mode
        S.demo = null
        S.notice = null
      }
      emit()
      return reply()
    },
    async setFeeder(running: boolean) {
      await guard()
      S.feeder.running = running
      emit()
      return reply()
    },
    subscribe(sub) {
      listeners.add(sub)
      sub.onConnection(!offline)
      if (!offline) sub.onState(structuredClone(S))
      return () => listeners.delete(sub)
    },
  }

  ;(window as unknown as { __studioMock: unknown }).__studioMock = {
    setOffline(v: boolean) {
      offline = v
      listeners.forEach((l) => l.onConnection(!v))
      if (!v) emit()
    },
    state: S,
  }
  return api
}
