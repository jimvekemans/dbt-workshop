import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Slider, trackFraction } from './Slider'
import { DayPicker } from './DayPicker'
import { Toggle } from './Toggle'
import { InputField } from './InputField'
import { ModeDialog } from './ModeDialog'
import { baseServer, mount } from '../test/harness'
import type { StepInput } from '../api/types'

describe('Slider: a click on the track', () => {
  it('maps a pointer position to a position along the track, leaving room for the thumb', () => {
    expect(trackFraction(0, 0, 300, 16)).toBe(0)
    expect(trackFraction(300, 0, 300, 16)).toBe(1)
    expect(trackFraction(150, 0, 300, 16)).toBeCloseTo(0.5, 2)
    expect(trackFraction(10, 0, 10, 16)).toBe(0)
  })
  it('moves the value where the track was pressed (full control height), not only with the keys', () => {
    const onChange = vi.fn()
    render(<Slider label="Days" value={0} min={0} max={10} onChange={onChange} />)
    const s = screen.getByRole('slider')
    s.getBoundingClientRect = () => ({ left: 100, width: 400, top: 0, right: 500, bottom: 44, height: 44, x: 100, y: 0, toJSON: () => ({}) })
    fireEvent.pointerDown(s, { clientX: 300, button: 0 })
    expect(onChange).toHaveBeenCalledWith(5)
  })
  it('ignores a press while disabled', () => {
    const onChange = vi.fn()
    render(<Slider label="Days" value={0} min={0} max={10} onChange={onChange} disabled />)
    fireEvent.pointerDown(screen.getByRole('slider'), { clientX: 300, button: 0 })
    expect(onChange).not.toHaveBeenCalled()
  })
})

describe('DayPicker: today, how long ago, the marked day', () => {
  const props = { label: 'Switch on', min: '2026-03-11', max: '2026-03-18', value: '2026-03-14', onChange: () => {} }
  it('labels the last day "vandaag", says "4 dagen geleden", and keeps its text readable when disabled', () => {
    render(<DayPicker {...props} disabled />)
    expect(screen.getByText('vandaag')).toBeInTheDocument()
    expect(screen.getByText('4 dagen geleden')).toBeInTheDocument()
    expect(screen.getByText('zaterdag')).toBeInTheDocument()
    expect(screen.getByRole('slider')).toHaveAttribute('aria-valuetext', 'zaterdag 14 maart 2026, 4 dagen geleden')
  })
  it('shows the tick for the day the scenario names, with its words', () => {
    render(<DayPicker {...props} mark={{ day: '2026-03-14', label: 'The real change day' }} />)
    expect(screen.getByText(/The real change day/)).toBeInTheDocument()
  })
  it('shows no tick when the contract sends none', () => {
    render(<DayPicker {...props} />)
    expect(screen.queryByText(/▲/)).toBeNull()
  })
})

describe('Toggle and InputField', () => {
  it('has a name without its state; the state is shown beside it and is not read twice', () => {
    render(<Toggle label="Send the mail" checked={false} onChange={() => {}} />)
    expect(screen.getByRole('switch', { name: 'Send the mail' })).toBeInTheDocument()
    expect(screen.getByText('Uit')).toHaveAttribute('aria-hidden', 'true')
  })
  const toggle: StepInput = { id: 'f', type: 'toggle', label: 'Extra check', help: { on: 'It runs.', off: 'It is skipped.' }, default: false }
  it('shows the help for the current state of a toggle', () => {
    const { rerender } = render(<InputField input={toggle} value={false} onChange={() => {}} />)
    expect(screen.getByText('It is skipped.')).toBeInTheDocument()
    rerender(<InputField input={toggle} value={true} onChange={() => {}} />)
    expect(screen.getByText('It runs.')).toBeInTheDocument()
  })
  it('shows the help line of a choice', () => {
    const choice: StepInput = { id: 'c', type: 'choice', label: 'Who', help: 'Pick the operator.', default: 'a', options: [{ value: 'a', label: 'A' }, { value: 'b', label: 'B' }] }
    render(<InputField input={choice} value="a" onChange={() => {}} />)
    expect(screen.getByText('Pick the operator.')).toBeInTheDocument()
  })
  it('in replay: disabled (the one line about the fixed value is the step card\'s)', () => {
    const num: StepInput = { id: 'n', type: 'number', label: 'Days late', default: 3, min: 0, max: 9, step: 1, unit: 'days' }
    render(<InputField input={num} value={3} onChange={() => {}} replay />)
    expect(screen.getByRole('slider')).toHaveAttribute('disabled')
  })
})

describe('ModeDialog: the mouse works on the visible text', () => {
  it('switches to the recorded replay by clicking its text', async () => {
    const { push, backend } = mount(<ModeDialog open onClose={() => {}} />, null)
    await push(baseServer)
    expect(screen.getByRole('dialog', { name: 'Wijzig hoe de demo\'s draaien' })).toBeInTheDocument()
    fireEvent.click(screen.getByText('Opname'))
    expect(backend.setMode).toHaveBeenCalledWith('replay')
    expect(screen.getByText(/Elke knop doet het echte werk/)).toBeInTheDocument()
  })
  it('cannot be switched while work is running, and says so', async () => {
    const { push } = mount(<ModeDialog open onClose={() => {}} />, null)
    await push({ ...baseServer, job: { kind: 'step', scenario: 'a', step: 'b', label: 'x', started: 1, cancellable: true } })
    expect(screen.getByText('Je kan wisselen zodra het huidige werk klaar is.')).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: 'Live' }).closest('fieldset')).toHaveAttribute('disabled')
  })
})
