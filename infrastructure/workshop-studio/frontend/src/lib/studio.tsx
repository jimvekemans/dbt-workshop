import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef } from 'react'
import type { ReactNode } from 'react'
import { ApiError } from '../api/types'
import type { Backend, InputValues, Mode, Scenario, ServerState } from '../api/types'
import { initialState, reducer } from './store'
import type { AppState } from './store'
import { useToast } from '../components/Toast'

export interface StudioActions {
  prepare: (id: string) => Promise<boolean>
  runStep: (id: string, step: string, inputs: InputValues) => Promise<boolean>
  cleanup: () => Promise<boolean>
  cancel: () => Promise<boolean>
  setMode: (mode: Mode) => Promise<boolean>
  setFeeder: (running: boolean) => Promise<boolean>
  loadScenario: (id: string) => void
  retryScenario: (id: string) => void
  retryCatalog: () => void
  /** Ask the studio right now (the connection-lost banner's "Try now"). */
  retryConnection: () => void
}
interface Studio extends AppState {
  actions: StudioActions
}

const Ctx = createContext<Studio | null>(null)

export function useStudio(): Studio {
  const v = useContext(Ctx)
  if (!v) throw new Error('useStudio must be used inside <StudioProvider>')
  return v
}

export function useScenario(id: string): { scenario: Scenario | null; error: string | null; notFound: boolean } {
  const { scenarios, scenarioErrors, scenarioNotFound, actions } = useStudio()
  const scenario = scenarios[id] ?? null
  const error = scenarioErrors[id] ?? null
  useEffect(() => {
    if (!scenario && !error) actions.loadScenario(id)
  }, [id, scenario, error, actions])
  return { scenario, error, notFound: !!scenarioNotFound[id] }
}

export function StudioProvider({ backend, children }: { backend: Backend; children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, initialState)
  const toast = useToast()
  const loading = useRef(new Set<string>())

  const loadCatalog = useCallback(() => {
    backend.getCatalog().then(
      (catalog) => dispatch({ type: 'catalog', catalog }),
      (e: unknown) => dispatch({ type: 'catalogFailed', message: e instanceof Error ? e.message : String(e) }),
    )
  }, [backend])

  useEffect(() => {
    loadCatalog()
    backend.getState().then(
      (server) => dispatch({ type: 'server', server }),
      () => dispatch({ type: 'connection', connected: false }),
    )
    return backend.subscribe({
      onState: (server: ServerState) => dispatch({ type: 'server', server }),
      onConnection: (connected) => dispatch({ type: 'connection', connected }),
    })
  }, [backend, loadCatalog])

  const act = useCallback(
    async (fn: () => Promise<ServerState>): Promise<boolean> => {
      try {
        dispatch({ type: 'server', server: await fn() })
        return true
      } catch (e) {
        toast.show(e instanceof ApiError || e instanceof Error ? e.message : 'Something went wrong.', 'bad')
        return false
      }
    },
    [toast],
  )

  const actions = useMemo<StudioActions>(
    () => ({
      prepare: (id) => act(() => backend.prepare(id)),
      runStep: (id, step, inputs) => act(() => backend.runStep(id, step, inputs)),
      cleanup: () => act(() => backend.cleanup()),
      cancel: () => act(() => backend.cancel()),
      setMode: (mode) => act(() => backend.setMode(mode)),
      setFeeder: (running) => act(() => backend.setFeeder(running)),
      retryScenario: (id) => dispatch({ type: 'scenarioRetry', id }),
      retryCatalog: () => {
        loadCatalog()
      },
      retryConnection: () => {
        backend.getState().then(
          (server) => {
            dispatch({ type: 'connection', connected: true })
            dispatch({ type: 'server', server })
          },
          () => undefined,
        )
      },
      loadScenario: (id) => {
        if (loading.current.has(id)) return
        loading.current.add(id)
        backend.getScenario(id).then(
          (scenario) => {
            loading.current.delete(id)
            dispatch({ type: 'scenario', scenario })
          },
          (e: unknown) => {
            loading.current.delete(id)
            dispatch({ type: 'scenarioFailed', id, message: e instanceof Error ? e.message : String(e), notFound: e instanceof ApiError && e.status === 404 })
          },
        )
      },
    }),
    [act, backend, loadCatalog],
  )

  const value = useMemo(() => ({ ...state, actions }), [state, actions])
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}
