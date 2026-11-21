import { describe, expect, it } from 'vitest'
import { demoFor, flatScenarios, initialState, neighbours, reducer, searchScenarios, stepBlockedReason } from './store'
import type { Catalog, Scenario, ServerState } from '../api/types'
import { SCENARIOS } from '../dev/mockContent'

const sc = (id: string, title: string) => ({ id, title, promise: `Promise of ${title}`, minutes: 3, environment: 'dev' as const, related: [], recorded: id !== 'b' })
const catalog: Catalog = {
  glossary: {},
  groups: [
    { id: 'g1', title: 'First group', blurb: '', scenarios: [sc('a', 'Alpha mail'), sc('b', 'Beta release')] },
    { id: 'g2', title: 'Second group', blurb: '', scenarios: [sc('c', 'Gamma')] },
  ],
}
const server = (over: Partial<ServerState> = {}): ServerState => ({
  mode: 'live', live_unavailable: null, notice: null, stack: [], baseline: { status: 'ready', detail: '', progress: [] }, feeder: { running: true, simulated_day: null, last_delivery: null },
  job: null, leftovers: [], demo: null, ...over,
})

describe('reducer', () => {
  it('stores the catalog, scenarios and server state', () => {
    let s = reducer(initialState, { type: 'catalog', catalog })
    expect(s.catalog).toBe(catalog)
    s = reducer(s, { type: 'scenario', scenario: { id: 'a' } as Scenario })
    expect(s.scenarios.a).toBeDefined()
    s = reducer(s, { type: 'server', server: server() })
    expect(s.server?.mode).toBe('live')
  })
  it('tracks the connection flag without needless new objects', () => {
    const s = reducer(initialState, { type: 'connection', connected: false })
    expect(s.connected).toBe(false)
    expect(reducer(s, { type: 'connection', connected: false })).toBe(s)
  })
  it('records and clears a scenario load error', () => {
    let s = reducer(initialState, { type: 'scenarioFailed', id: 'a', message: 'nope' })
    expect(s.scenarioErrors.a).toBe('nope')
    s = reducer(s, { type: 'scenarioRetry', id: 'a' })
    expect(s.scenarioErrors.a).toBeUndefined()
  })
})

describe('selectors', () => {
  it('flattens, finds neighbours and searches', () => {
    expect(flatScenarios(catalog).map((s) => s.id)).toEqual(['a', 'b', 'c'])
    expect(neighbours(catalog, 'b')).toMatchObject({ prev: { id: 'a' }, next: { id: 'c' }, index: 1, total: 3 })
    expect(neighbours(catalog, 'a').prev).toBeNull()
    expect(searchScenarios(catalog, 'mail').map((s) => s.id)).toEqual(['a'])
    expect(searchScenarios(catalog, 'second').map((s) => s.id)).toEqual(['c'])
    expect(searchScenarios(catalog, '  ')).toHaveLength(3)
  })
  it('only returns the demo of the asked scenario', () => {
    const demo = { scenario: 'a', status: 'ready' as const, message: '', steps: [] }
    expect(demoFor(server({ demo }), 'a')).toBe(demo)
    expect(demoFor(server({ demo }), 'b')).toBeNull()
  })
  it('explains why a step cannot run', () => {
    const scenario = SCENARIOS['switchover-date']!
    const entry = flatScenarios(catalog)[0]!
    expect(stepBlockedReason(null, scenario, entry, 0)).toMatch(/Wachten/)
    expect(stepBlockedReason(server(), scenario, entry, 0)).toMatch(/Zet eerst de demo klaar/)
    const steps = scenario.demo.steps.map((s) => ({ id: s.id, status: 'todo' as const, actions: [], evidence: [] }))
    const open = server({ demo: { scenario: scenario.id, status: 'ready', steps } })
    expect(stepBlockedReason(open, scenario, entry, 0)).toBeNull()
    expect(stepBlockedReason(open, scenario, entry, 1)).toMatch(/Rond eerst/)
    const busy = server({ ...open, job: { kind: 'step', scenario: scenario.id, step: 'without', label: 'Running the load', started: 0, cancellable: true } })
    expect(stepBlockedReason(busy, scenario, entry, 0)).toMatch(/Wacht: "Running the load" loopt nog/)
    const replay = server({ ...open, mode: 'replay' })
    expect(stepBlockedReason(replay, scenario, { ...entry, recorded: false }, 0)).toMatch(/Geen opname voor deze stap/)
  })
})
