import { plainText } from './glossary'
import { nextNumberTrails, nextVersionBaselines } from '../api/adapt'
import type { NumberTrails, VersionTags } from '../api/adapt'
import type { Catalog, CatalogScenario, Demo, DemoStep, Scenario, ServerState } from '../api/types'

export interface AppState {
  catalog: Catalog | null
  catalogError: string | null
  scenarios: Record<string, Scenario>
  scenarioErrors: Record<string, string>
  /** The server said 404: the scenario does not exist (an old or mistyped link). */
  scenarioNotFound: Record<string, true>
  server: ServerState | null
  /** The version of each environment when a step started, per versions item (see nextVersionBaselines). */
  versionBaselines: Record<string, VersionTags>
  /** The values a number item showed while its step ran, per item (see nextNumberTrails). */
  numberTrails: NumberTrails
  connected: boolean
}

export type Action =
  | { type: 'catalog'; catalog: Catalog }
  | { type: 'catalogFailed'; message: string }
  | { type: 'scenario'; scenario: Scenario }
  | { type: 'scenarioFailed'; id: string; message: string; notFound?: boolean }
  | { type: 'scenarioRetry'; id: string }
  | { type: 'server'; server: ServerState }
  | { type: 'connection'; connected: boolean }

export const initialState: AppState = {
  catalog: null,
  catalogError: null,
  scenarios: {},
  scenarioErrors: {},
  scenarioNotFound: {},
  server: null,
  versionBaselines: {},
  numberTrails: {},
  connected: true,
}

export function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'catalog':
      return { ...state, catalog: action.catalog, catalogError: null }
    case 'catalogFailed':
      return { ...state, catalogError: action.message }
    case 'scenario': {
      const { [action.scenario.id]: _gone, ...errors } = state.scenarioErrors
      void _gone
      return { ...state, scenarios: { ...state.scenarios, [action.scenario.id]: action.scenario }, scenarioErrors: errors }
    }
    case 'scenarioFailed':
      return {
        ...state,
        scenarioErrors: { ...state.scenarioErrors, [action.id]: action.message },
        scenarioNotFound: action.notFound ? { ...state.scenarioNotFound, [action.id]: true } : state.scenarioNotFound,
      }
    case 'scenarioRetry': {
      const { [action.id]: _gone, ...errors } = state.scenarioErrors
      void _gone
      const { [action.id]: _nf, ...missing } = state.scenarioNotFound
      void _nf
      return { ...state, scenarioErrors: errors, scenarioNotFound: missing }
    }
    case 'server':
      return { ...state, server: action.server, versionBaselines: nextVersionBaselines(state.versionBaselines, action.server.demo), numberTrails: nextNumberTrails(state.numberTrails, action.server.demo) }
    case 'connection':
      return state.connected === action.connected ? state : { ...state, connected: action.connected }
  }
}

// ------------------------------------------------------------------ selectors
export function flatScenarios(catalog: Catalog | null): (CatalogScenario & { groupId: string; groupTitle: string })[] {
  if (!catalog) return []
  return catalog.groups.flatMap((g) => g.scenarios.map((s) => ({ ...s, groupId: g.id, groupTitle: g.title })))
}

export function findCatalogScenario(catalog: Catalog | null, id: string) {
  return flatScenarios(catalog).find((s) => s.id === id) ?? null
}

export function neighbours(catalog: Catalog | null, id: string) {
  const all = flatScenarios(catalog)
  const i = all.findIndex((s) => s.id === id)
  return { prev: i > 0 ? (all[i - 1] ?? null) : null, next: i >= 0 ? (all[i + 1] ?? null) : null, index: i, total: all.length }
}

export function searchScenarios(catalog: Catalog | null, query: string) {
  const q = query.trim().toLowerCase()
  const all = flatScenarios(catalog)
  if (!q) return all
  const words = q.split(/\s+/)
  return all.filter((s) => {
    // the technical keywords of the scenario and of its steps count too, whether or not they are shown on the screen
    const tech = [...(s.tech ?? []), ...(s.steps ?? []).map((st) => st.tech ?? '')].join(' ')
    const hay = `${plainText(`${s.title} ${s.promise} ${s.groupTitle}`)} ${tech}`.toLowerCase()
    return words.every((w) => hay.includes(w))
  })
}

export function demoFor(server: ServerState | null, scenarioId: string): Demo | null {
  return server?.demo && server.demo.scenario === scenarioId ? server.demo : null
}

export function stepState(demo: Demo | null, stepId: string): DemoStep | null {
  return demo?.steps.find((s) => s.id === stepId) ?? null
}

export function isBusy(server: ServerState | null): boolean {
  return !!server?.job
}

/** The demo of this scenario failed to open, or is being made ready (the stack is checked first). */
export function demoIsPreparing(demo: Demo | null, server: ServerState | null): boolean {
  return demo?.status === 'preparing' || (server?.job?.kind === 'prepare' && (server.job.scenario === null || demo?.scenario === server.job.scenario))
}

export function demoIsOpen(demo: Demo | null): boolean {
  return !!demo && (demo.status === 'ready' || demo.status === 'running')
}

/** null when the step can run, otherwise the plain reason why not (shown as a title and as a visible hint). */
export function stepBlockedReason(
  server: ServerState | null,
  scenario: Scenario,
  catalogEntry: CatalogScenario | null,
  index: number,
): string | null {
  if (!server) return 'Wachten tot de Studio antwoordt.'
  if (server.job) return server.job.kind === 'step' ? `Wacht: "${server.job.label}" loopt nog.` : `Wacht tot dit klaar is: ${server.job.label}.`
  const demo = demoFor(server, scenario.id)
  if (!demoIsOpen(demo)) return 'Zet eerst de demo klaar.'
  if (server.mode === 'replay' && catalogEntry && !catalogEntry.recorded) {
    return 'Geen opname voor deze stap.'
  }
  if (index > 0) {
    const prev = scenario.demo.steps[index - 1]
    if (prev && stepState(demo, prev.id)?.status !== 'done') return `Rond eerst "${plainText(prev.title)}" af.`
  }
  return null
}
