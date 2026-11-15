import { describe, expect, it } from 'vitest'
import { act, fireEvent, screen } from '@testing-library/react'
import { useState } from 'react'
import { StepCard, buttonLabel, finalEvidence } from './StepCard'
import { DemoPart } from './DemoPart'
import { baseServer, mount } from '../test/harness'
import { SCENARIOS } from '../dev/mockContent'
import type { DemoStep, InputValues, LiveEvidence, StepDef } from '../api/types'

const sc = SCENARIOS['switchover-date']!
const def: StepDef = { ...sc.demo.steps[1]!, usual_seconds: 23, inputs: [{ id: 'switch_day', type: 'day', label: 'Switch day', default: '2026-11-11', min: '2026-11-08', max: '2026-11-15', mark: { day: '2026-11-11', label: 'The real day' } }] }
const step = (over: Partial<DemoStep> = {}): DemoStep => ({ id: def.id, status: 'todo', actions: [], evidence: def.show.map((e) => ({ ...e, data: null })), ...over })
const card = (state: DemoStep | null, extra: Partial<Parameters<typeof StepCard>[0]> = {}) => (
  <StepCard scenarioId={sc.id} index={1} total={2} def={def} state={state} blocked={null} values={{ switch_day: '2026-11-11' } as InputValues} onValue={() => undefined} onRun={() => undefined} {...extra} />
)

describe('StepCard outcomes', () => {
  it('a good outcome is the green "What just happened"', () => {
    mount(card(step({ status: 'done', outcome: 'It worked.' })), null)
    expect(screen.getByText('What just happened')).toBeInTheDocument()
    expect(screen.queryByText('Not quite')).toBeNull()
  })
  it('"not_quite" is the amber box titled "Not quite"', () => {
    mount(card(step({ status: 'done', outcome: 'Two days are still unreadable.', outcome_kind: 'not_quite' })), null)
    expect(screen.getByText('Not quite')).toBeInTheDocument()
    expect(screen.queryByText('What just happened')).toBeNull()
    expect(screen.getByText('Two days are still unreadable.')).toBeInTheDocument()
  })
})

describe('StepCard usual duration', () => {
  it('says "usually about 25 s" before and while the step runs', () => {
    const { container } = mount(card(step()), null)
    expect(screen.getByText('Usually about 25 s')).toBeInTheDocument()
    expect(container).toBeDefined()
  })
  it('shows it next to "Happening now" while running', () => {
    mount(card(step({ status: 'running' })), null)
    expect(screen.getByText('usually about 25 s')).toBeInTheDocument()
  })
  it('says nothing when the duration is not known', () => {
    mount(card(step(), { def: { ...def, usual_seconds: null } }), null)
    expect(screen.queryByText(/sually about/)).toBeNull()
  })
})

describe('StepCard when it failed or was stopped', () => {
  it('shows only the red box and "Nothing to show yet", without stale evidence', () => {
    const ev = def.show.map((e) => ({ ...e, data: { summary: 'This worked earlier', data: {} } }))
    const { container } = mount(card(step({ status: 'failed', summary: 'The check found a problem.', evidence: ev as never })), null)
    expect(screen.getByRole('alert')).toHaveTextContent('Nothing is broken: press “Try this step again”.')
    expect(screen.getByText('Nothing to show yet: the step stopped before it could finish.')).toBeInTheDocument()
    expect(container.querySelectorAll('section h4')).toHaveLength(0)
  })
  it('a stopped step is grey "Stopped" and says how to start again', () => {
    mount(card(step({ status: 'stopped' })), null)
    expect(screen.getByText('Stopped')).toBeInTheDocument()
    expect(screen.getByText(/Stopped\. Nothing was lost; press “Apply the day and run the nightly load” to start again\./)).toBeInTheDocument()
    expect(screen.getByText('Appears after you press the button above.')).toBeInTheDocument()
  })
  it('a locked step names the step that is still running', () => {
    mount(card(step(), { blocked: 'Wait: “Now choose” is still running.' }), null)
    expect(screen.getAllByText('Wait: “Now choose” is still running.').length).toBeGreaterThan(0)
  })
})

describe('StepCard in replay', () => {
  it('says "Play the recording: ...", fixes the inputs, and marks every frame with the recording date', () => {
    const ev = def.show.map((e) => ({ ...e, recorded_at: '2026-11-10T09:30:00', data: { labels: ['a'], x_label: '', series: [{ name: 'readable', state: 'good', values: [1] }] } }))
    mount(card(step({ status: 'done', evidence: ev as never }), { replay: true }), null)
    expect(screen.getByText('Play the recording again')).toBeInTheDocument()
    expect(screen.getByText(/Fixed in the recording: Wednesday 11 November 2026\. Switch to Live to choose yourself\./)).toBeInTheDocument()
    expect(screen.getByRole('slider')).toHaveAttribute('disabled')
    expect(screen.getByText(/Recorded 10 Nov/)).toBeInTheDocument()
  })
  it('names the button of a recording', () => {
    expect(buttonLabel(def, 'todo', true)).toBe('Play the recording: Apply the day and run the nightly load')
    expect(buttonLabel(def, 'todo', false)).toBe('Apply the day and run the nightly load')
    expect(buttonLabel(def, 'failed', true)).toBe('Try this step again')
  })
})

describe('StepCard "Before"', () => {
  it('shows the earlier result of the same kind above the new one, inert', () => {
    const bars = { labels: ['a'], x_label: '', series: [{ name: 'readable', state: 'good' as const, values: [1] }] }
    const first = { ...def.show[0]!, data: bars } as LiveEvidence
    const { container } = mount(
      card(step({ status: 'done' }), {
        def: { ...def, show: [{ ...def.show[0]!, before_of: 'without' }] },
        beforeFor: (id, type) => (id === 'without' && type === 'bars' ? first : null),
      }),
      null,
    )
    expect(screen.getByText('Before')).toBeInTheDocument()
    expect(container.querySelector('[inert]')).not.toBeNull()
  })
  it('finalEvidence picks the last item of a kind that has data', () => {
    const s = step({ evidence: [{ type: 'bars', data: null }, { type: 'bars', data: { labels: [], series: [] } as never }] as never })
    expect(finalEvidence(s, 'bars')).toBe(s.evidence[1])
    expect(finalEvidence(null, 'bars')).toBeNull()
  })
})

describe('StepCard focus', () => {
  it('moves focus to the heading when the step starts and to the result when it ends', () => {
    let set: (s: DemoStep) => void = () => undefined
    function Host() {
      const [s, setS] = useState<DemoStep>(step())
      set = setS
      return card(s)
    }
    mount(<Host />, null)
    act(() => set(step({ status: 'running' })))
    expect(screen.getByRole('heading', { name: def.title })).toHaveFocus()
    act(() => set(step({ status: 'done', outcome: 'Finished fine.' })))
    expect(screen.getByText('What just happened').closest('[role="status"]')).toHaveFocus()
  })
})

describe('DemoPart: replay banner and the end of the demo', () => {
  const done = sc.demo.steps.map((s) => ({ id: s.id, status: 'done' as const, actions: [], evidence: s.show.map((e) => ({ ...e, data: null })) }))
  const open = { scenario: sc.id, status: 'ready' as const, message: '', steps: done }

  it('has a "Switch to Live" button in the replay banner', async () => {
    const { push, backend } = mount(<DemoPart scenario={sc} />, null)
    await push({ ...baseServer, mode: 'replay' })
    fireEvent.click(await screen.findByRole('button', { name: 'Switch to Live' }))
    expect(backend.setMode).toHaveBeenCalledWith('live')
  })

  it('ends neutrally: what was changed, "Put everything back", and an invitation to try it', async () => {
    const { push, backend } = mount(<DemoPart scenario={sc} />, null)
    await push({ ...baseServer, demo: open, leftovers: [{ label: 'The practice data changed by “X”', from_scenario: sc.id }] })
    expect(await screen.findByText('That is the whole demo')).toBeInTheDocument()
    expect(screen.getByText(/This demo changed the practice system a little \(The practice data changed by “X”\)/)).toBeInTheDocument()
    expect(screen.getByText(/Try it yourself: change the/)).toBeInTheDocument()
    expect(screen.queryByText(/cleaned up for you/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Put everything back' }))
    expect(backend.cleanup).toHaveBeenCalled()
  })

  it('says it left things as they were when nothing is left over', async () => {
    const { push } = mount(<DemoPart scenario={sc} />, null)
    await push({ ...baseServer, demo: open })
    expect(await screen.findByText('This demo left the practice system as it was.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Put everything back' })).toBeNull()
  })
})
