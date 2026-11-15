import { describe, expect, it } from 'vitest'
import { fireEvent, screen } from '@testing-library/react'
import { StatusRow } from './StatusRow'
import { baseServer, mount } from '../test/harness'
import type { ServerState, StackService } from '../api/types'

const svc = (id: string, status: StackService['status'] = 'up'): StackService => ({ id, label: `Label ${id}`, status, detail: `Detail of ${id}` })
const withStack = (over: Partial<ServerState> = {}): ServerState => ({ ...baseServer, stack: [svc('gitlab'), svc('airflow-val'), svc('mailpit')], ...over })
const details = () => screen.getByRole('button', { name: /Details/ })

describe('StatusRow', () => {
  it('is one quiet row: everything running, Details closed, the mode and a way to change it', async () => {
    const { push } = mount(<StatusRow />, null)
    await push(withStack())
    expect(screen.getByText('Demo system: ready')).toBeInTheDocument()
    expect(details()).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByText('Demos run live.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Change demo mode' })).toBeInTheDocument()
    expect(screen.queryByText('Code storage (GitLab)')).toBeNull()
  })

  it('names the services plainly, with each detail sentence, once opened', async () => {
    const { push } = mount(<StatusRow />, null)
    await push(withStack())
    fireEvent.click(details())
    expect(screen.getByText('Code storage (GitLab)')).toBeInTheDocument()
    expect(screen.getByText('Nightly runs: validation')).toBeInTheDocument()
    expect(screen.getByText('Test mailbox')).toBeInTheDocument()
    expect(screen.getByText('Detail of airflow-val')).toBeInTheDocument()
    expect(screen.getByText('Practice data keeps coming in')).toBeInTheDocument()
    expect(screen.getByText(/Nothing left over from earlier demos/)).toBeInTheDocument()
    expect(screen.getByText(/Open the system overview \(new tab\)/)).toBeInTheDocument()
  })

  it('says the demos play from a recording in the replay mode', async () => {
    const { push } = mount(<StatusRow />, null)
    await push(withStack({ mode: 'replay' }))
    expect(screen.getByText('Demos play from a recording.')).toBeInTheDocument()
  })

  it('opens Details by itself when a service is not running, and says how many', async () => {
    const { push } = mount(<StatusRow />, null)
    await push(withStack({ stack: [svc('gitlab'), svc('airflow-val', 'down'), svc('mailpit')] }))
    expect(screen.getByText('1 of 3 things the demos need are not running')).toBeInTheDocument()
    expect(details()).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('Not running')).toBeInTheDocument()
  })

  it('opens Details while the practice system is not ready, shows its progress, and closes again when it is ready', async () => {
    const { push } = mount(<StatusRow />, null)
    const progress = [{ label: 'Demo data in dev', status: 'running' as const }, { label: 'Tables built once in prd', status: 'todo' as const }]
    await push(withStack({ baseline: { status: 'preparing', detail: 'Checking the demo stack.', progress } }))
    expect(screen.getByText('Getting the practice system ready: Demo data in development')).toBeInTheDocument()
    expect(details()).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('Tables built once in production')).toBeInTheDocument()
    await push(withStack())
    expect(details()).toHaveAttribute('aria-expanded', 'false')
  })

  it('opens Details when something is left over and offers a neutral clean up', async () => {
    const { push, backend } = mount(<StatusRow />, null)
    await push(withStack({ leftovers: [{ label: 'The practice data changed by “X”', from_scenario: 'switchover-date' }] }))
    expect(details()).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('The last demo left its practice data behind')).toBeInTheDocument()
    expect(screen.getByText(/Harmless/)).toBeInTheDocument()
    expect(screen.getByText('What exactly?')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Clean up now' }))
    expect(backend.cleanup).toHaveBeenCalled()
  })

  it('leaves it closed if the presenter closed it, even when the problem goes away', async () => {
    const { push } = mount(<StatusRow />, null)
    await push(withStack({ stack: [svc('gitlab', 'down')] }))
    fireEvent.click(details())
    expect(details()).toHaveAttribute('aria-expanded', 'false')
    await push(withStack({ stack: [svc('gitlab', 'down')] }))
    expect(details()).toHaveAttribute('aria-expanded', 'false')
  })

  it('says the status is unknown while offline', async () => {
    const { push, connection } = mount(<StatusRow />, null)
    await push(withStack())
    await connection(false)
    expect(screen.getByText('Status unknown while offline')).toBeInTheDocument()
  })
})
