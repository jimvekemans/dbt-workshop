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
    expect(d.text).toBe(`Stap 1 loopt: ${sc.demo.steps[0]!.title}`)
    expect(d.usual).toBe('meestal ongeveer 25 s')
    expect(d.stop).toBe('Stop deze stap')
    expect(d.confirm).toBe('Deze stap stoppen?')
  })
  it('falls back to the job label when the scenario is not loaded', () => {
    expect(describeJob(job({ label: 'Running the load' }), {}).text).toBe('Bezig: Running the load')
  })
  it('has its own words for getting ready and for clean up', () => {
    expect(describeJob(job({ kind: 'prepare', step: null }), {}).text).toBe('De demo wordt klaargezet')
    expect(describeJob(job({ kind: 'prepare', scenario: null, step: null }), {}).text).toBe('Het oefensysteem wordt klaargezet')
    expect(describeJob(job({ kind: 'prepare', step: null }), {}).stop).toBe('Stop het klaarzetten')
    expect(describeJob(job({ kind: 'cleanup', scenario: null, step: null }), {}).text).toBe('Alles wordt opgekuist')
  })
})

describe('JobIndicator', () => {
  it('renders nothing when the studio is idle', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push(baseServer)
    expect(screen.queryByText(/loopt|Bezig|klaargezet|opgekuist/)).toBeNull()
  })

  it('is one fixed pill with the step, the seconds hidden from screen readers, and a Stop button', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ label: sc.demo.steps[0]!.title }) })
    const pill = screen.getByText(/Bezig: /).closest('[role="status"]')!
    expect(pill).toHaveTextContent('Bezig: ')
    expect([...pill.querySelectorAll('[aria-hidden="true"]')].map((e) => e.textContent).join('')).toMatch(/\d+ s/)
    expect(screen.getByRole('button', { name: 'Stop deze stap' })).toBeInTheDocument()
    expect(pill.closest('header')).toBeNull()
  })

  it('has no stop button when the job cannot be stopped', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ cancellable: false }) })
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull()
  })

  it('asks first, with "Ga verder" focused, and stops only on "Stop de stap"', async () => {
    const { push, backend } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ label: 'Switch day' }) })
    fireEvent.click(screen.getByRole('button', { name: 'Stop deze stap' }))
    const dialog = screen.getByRole('dialog', { name: 'Deze stap stoppen?' })
    expect(dialog).toHaveTextContent('Wat ze al wijzigde in de oefendata, wordt automatisch opgekuist')
    expect(screen.getByRole('button', { name: 'Ga verder' })).toHaveFocus()
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(backend.cancel).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Stop deze stap' }))
    fireEvent.click(screen.getByRole('button', { name: 'Stop de stap' }))
    expect(backend.cancel).toHaveBeenCalledTimes(1)
  })
})

/** The stylesheet as text (read from the project folder, where vitest runs; no node types in this project, hence the computed module name). */
const readCss = async (): Promise<string> => {
  const fs = (await import(/* @vite-ignore */ ['node', 'fs'].join(':'))) as { readFileSync(p: string, enc: string): string }
  return fs.readFileSync('src/components/JobIndicator.module.css', 'utf8')
}

describe('the pill and its stop button (markup)', () => {
  it('has the button inside the pill, and marks the pill that carries one', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, job: job({ label: 'x' }) })
    const pill = document.querySelector('.job')!
    const stop = screen.getByRole('button', { name: 'Stop deze stap' })
    expect(pill.contains(stop)).toBe(true)
    expect(pill).toHaveAttribute('data-stoppable', '')
    expect(pill.querySelectorAll('button')).toHaveLength(1)
  })
  it('has no stop button and no mark while a recording plays, or when the job cannot be stopped', async () => {
    const { push } = mount(<JobIndicator />, null)
    await push({ ...baseServer, mode: 'replay', job: job({}) })
    expect(document.querySelector('.job')!.querySelector('button')).toBeNull()
    expect(document.querySelector('.job')!.hasAttribute('data-stoppable')).toBe(false)
    await push({ ...baseServer, job: job({ cancellable: false }) })
    expect(document.querySelector('.job')!.hasAttribute('data-stoppable')).toBe(false)
  })
  it('gives the button the radius that follows from the pill: its radius minus the border and the gap', async () => {
    const css = await readCss()
    // the pill's radius is built from the button's height, the gap and the border, so the button's own radius comes out as half its height
    expect(css).toMatch(/--pill-r:\s*calc\(var\(--btn-h\) \/ 2 \+ var\(--pill-gap\) \+ var\(--pill-border\)\)/)
    expect(css).toMatch(/\.job button\s*\{[^}]*border-radius:\s*calc\(var\(--pill-r\) - var\(--pill-border\) - var\(--pill-gap\)\)/)
    expect(css).toMatch(/border-radius:\s*var\(--pill-r\)/)
    // the same gap above, below and at the end of the button
    expect(css).toMatch(/\.job \{[^}]*padding:\s*var\(--pill-gap\) 1\.1em var\(--pill-gap\) 1\.1em/)
    expect(css).toMatch(/\.withStop\s*\{\s*padding-right:\s*var\(--pill-gap\)/)
    expect(css).not.toMatch(/--radius-xl/)
  })
})
