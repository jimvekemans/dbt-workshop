import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { act } from 'react'
import { StudioProvider } from '../lib/studio'
import { ToastProvider } from '../components/Toast'
import { DemoPart } from './DemoPart'
import { CATALOG, SCENARIOS } from '../dev/mockContent'
import type { Backend, ServerState, Subscription } from '../api/types'

const scenario = SCENARIOS['switchover-date']!
const base: ServerState = {
  mode: 'live', live_unavailable: null, notice: null, stack: [], baseline: { status: 'ready', detail: '', progress: [] },
  feeder: { running: true, simulated_day: null, last_delivery: null }, job: null, leftovers: [], demo: null,
}

function setup(initial: ServerState) {
  let sub: Subscription | undefined
  const backend: Backend = {
    getCatalog: () => Promise.resolve(CATALOG),
    getScenario: () => Promise.resolve(scenario),
    getState: () => Promise.resolve(initial),
    prepare: vi.fn(() => Promise.resolve({ ...initial, job: { kind: 'prepare', scenario: scenario.id, step: null, label: 'Getting ready', started: 1, cancellable: true }, demo: { scenario: scenario.id, status: 'preparing', message: '', steps: [] } } as ServerState)),
    runStep: vi.fn(), cleanup: vi.fn(), cancel: vi.fn(), setMode: vi.fn(), setFeeder: vi.fn(),
    subscribe: (s) => { sub = s; return () => undefined },
  }
  render(
    <ToastProvider>
      <StudioProvider backend={backend}>
        <DemoPart scenario={scenario} />
      </StudioProvider>
    </ToastProvider>,
  )
  return { backend, push: (s: ServerState) => act(() => sub?.onState(s)) }
}

describe('DemoPart: Prepare this demo', () => {
  it('does nothing by itself, explains what getting ready does, and prepares only when the button is pressed', async () => {
    const { backend } = setup(base)
    const button = await screen.findByRole('button', { name: 'Prepare this demo' })
    expect(backend.prepare).not.toHaveBeenCalled()
    expect(screen.getByText(/starting state/)).toBeInTheDocument()
    await act(async () => { fireEvent.click(button) })
    expect(backend.prepare).toHaveBeenCalledTimes(1)
    expect(backend.prepare).toHaveBeenCalledWith(scenario.id)
  })

  it('stays disabled while another job runs', async () => {
    const { backend } = setup({ ...base, job: { kind: 'prepare', scenario: null, step: null, label: 'Checking that the demo stack is ready', started: 1, cancellable: true } })
    expect(await screen.findByText(/Checking the practice company/)).toBeInTheDocument()
    expect(backend.prepare).not.toHaveBeenCalled()
  })

  it('shows the failure sentence of the demo and lets the presenter try again', async () => {
    const { push } = setup(base)
    await screen.findByRole('button', { name: 'Prepare this demo' })
    push({ ...base, demo: { scenario: scenario.id, status: 'failed', message: 'Could not get this ready: Tables built once in val.', steps: [] } })
    expect(await screen.findByRole('alert')).toHaveTextContent('Tables built once in val.')
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument()
  })

  it('shows the steps once the demo is ready', async () => {
    const { push } = setup(base)
    await screen.findByRole('button', { name: 'Prepare this demo' })
    push({ ...base, demo: { scenario: scenario.id, status: 'ready', message: '', steps: scenario.demo.steps.map((s) => ({ id: s.id, status: 'todo' as const, summary: '', outcome: '', actions: [], evidence: s.show.map((e) => ({ ...e, data: null })) })) } })
    expect(await screen.findByRole('button', { name: scenario.demo.steps[0]!.button })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Prepare this demo' })).toBeNull()
  })
})
