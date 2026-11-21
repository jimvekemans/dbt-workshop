import { vi } from 'vitest'
import { act, render } from '@testing-library/react'
import type { ReactNode } from 'react'
import { ApiError } from '../api/types'
import type { Backend, Catalog, ServerState, Subscription } from '../api/types'
import { StudioProvider } from '../lib/studio'
import { ToastProvider } from '../components/Toast'
import { TermProvider } from '../components/Term'
import { CATALOG, SCENARIOS } from '../dev/mockContent'

export const baseServer: ServerState = {
  mode: 'live', live_unavailable: null, notice: null, stack: [], baseline: { status: 'ready', detail: '', progress: [] },
  feeder: { running: true, simulated_day: null, last_delivery: null }, job: null, leftovers: [], demo: null,
}

/** Renders a component inside the studio provider with a stub backend; `push` sends a server state, `connection` the link state. */
export function mount(ui: ReactNode, initial: ServerState | null = baseServer, catalog: Catalog = CATALOG) {
  let sub: Subscription | undefined
  const reply = () => Promise.resolve(initial ?? baseServer)
  const backend: Backend = {
    getCatalog: () => Promise.resolve(structuredClone(catalog)),
    getScenario: (id) => (SCENARIOS[id] ? Promise.resolve(structuredClone(SCENARIOS[id]!)) : Promise.reject(new ApiError('There is no such scenario.', 404))),
    getState: () => (initial ? Promise.resolve(initial) : new Promise(() => undefined)),
    prepare: vi.fn(reply), runStep: vi.fn(reply), cleanup: vi.fn(reply), cancel: vi.fn(reply), setMode: vi.fn(reply), setFeeder: vi.fn(reply),
    subscribe: (s) => {
      sub = s
      return () => undefined
    },
  }
  const view = render(
    <ToastProvider>
      <StudioProvider backend={backend}>
        <TermProvider glossary={CATALOG.glossary}>{ui}</TermProvider>
      </StudioProvider>
    </ToastProvider>,
  )
  return {
    ...view,
    backend,
    push: (s: ServerState) => act(async () => sub?.onState(s)),
    connection: (c: boolean) => act(async () => sub?.onConnection(c)),
  }
}
