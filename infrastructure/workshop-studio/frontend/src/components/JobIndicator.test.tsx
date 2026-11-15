import { describe, expect, it } from 'vitest'
import { fireEvent, screen } from '@testing-library/react'
import { JobIndicator, describeJob } from './JobIndicator'
import { baseServer, mount } from '../test/harness'
import { SCENARIOS } from '../dev/mockContent'
import type { Job } from '../api/types'

const sc = SCENARIOS['switchover-date']!
const job = (over: Partial<Job>): Job => ({ kind: 'step', scenario: sc.id, step: sc.demo.steps[0]!.id, label: 'x', started: Date.now() / 1000, cancellable: true, ...over })

describe('describeJob', () => {
  it('numbers the step and gives the usual duration when the scenario is known', () => {
    const steps = [{ ...sc.demo.steps[0]!, usual_seconds: 27 }, ...sc.demo.steps.slice(1)]
    const d = describeJob(job({}), { [sc.id]: { ...sc, demo: { ...sc.demo, steps } } })
    expect(d.text).toBe(`Step 1 is running: ${sc.demo.steps[0]!.title}`)
    expect(d.usual).toBe('usually about 25 s')
    expect(d.stop).toBe('Stop this step')
    expect(d.confirm).toBe('Stop this step?')
  })
  it('falls back to the job label when the scenario is not loaded', () => {
    expect(describeJob(job({ label: 'Running the load' }), {}).text).toBe('Running: Running the load')
  })
  it('has its own words for getting ready and for clean up', () => {
    expect(describeJob(job({ kind: 'prepare', step: null }), {}).text).toBe('Getting the demo ready')
    expect(describeJob(job({ kind: 'prepare', scenario: null, step: null }), {}).text).toBe('Getting the practice system ready')
    expect(describeJob(job({ kind: 'prepare', step: null }), {}).stop).toBe('Stop getting ready')
    expect(describeJob(job({ kind: 'cleanup', scenario: null, step: null }), {}).text).toBe('Putting everything back')
  })
})

describe('JobIndicator', () => {
  it('renders nothing when the studio is idle', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push(baseServer)
    expect(screen.queryByText(/running|Getting|Putting/)).toBeNull()
  })

  it('is one fixed pill with the step, the seconds hidden from screen readers, and a Stop button', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ label: sc.demo.steps[0]!.title }) })
    const pill = screen.getByText(/Running: /).closest('[role="status"]')!
    expect(pill).toHaveTextContent('Running: ')
    expect([...pill.querySelectorAll('[aria-hidden="true"]')].map((e) => e.textContent).join('')).toMatch(/\d+ s/)
    expect(screen.getByRole('button', { name: 'Stop this step' })).toBeInTheDocument()
    expect(pill.closest('header')).toBeNull()
  })

  it('has no stop button when the job cannot be stopped', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ cancellable: false }) })
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull()
  })

  it('asks first, with "Keep going" focused, and stops only on "Stop the step"', async () => {
    const { push, backend } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ label: 'Switch day' }) })
    fireEvent.click(screen.getByRole('button', { name: 'Stop this step' }))
    const dialog = screen.getByRole('dialog', { name: 'Stop this step?' })
    expect(dialog).toHaveTextContent('Anything it already changed in the practice data is tidied automatically')
    expect(screen.getByRole('button', { name: 'Keep going' })).toHaveFocus()
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(backend.cancel).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Stop this step' }))
    fireEvent.click(screen.getByRole('button', { name: 'Stop the step' }))
    expect(backend.cancel).toHaveBeenCalledTimes(1)
  })
})
