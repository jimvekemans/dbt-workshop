import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { DayPicker } from './DayPicker'
import { Slider } from './Slider'

describe('DayPicker', () => {
  const props = { label: 'Switch on', min: '2026-03-11', max: '2026-03-18', value: '2026-03-14' }
  it('spells the chosen date including the weekday', () => {
    render(<DayPicker {...props} onChange={() => {}} />)
    expect(screen.getByText('Saturday')).toBeInTheDocument()
    expect(screen.getByRole('slider')).toHaveAttribute('aria-valuetext', 'Saturday 14 March 2026, 4 days ago')
  })
  it('moves by one day with the arrow keys and jumps with Home and End', () => {
    const onChange = vi.fn()
    render(<DayPicker {...props} onChange={onChange} />)
    const slider = screen.getByRole('slider')
    fireEvent.keyDown(slider, { key: 'ArrowRight' })
    expect(onChange).toHaveBeenLastCalledWith('2026-03-15')
    fireEvent.keyDown(slider, { key: 'ArrowLeft' })
    expect(onChange).toHaveBeenLastCalledWith('2026-03-13')
    fireEvent.keyDown(slider, { key: 'Home' })
    expect(onChange).toHaveBeenLastCalledWith('2026-03-11')
    fireEvent.keyDown(slider, { key: 'End' })
    expect(onChange).toHaveBeenLastCalledWith('2026-03-18')
  })
  it('has day buttons that stop at the ends', () => {
    const onChange = vi.fn()
    render(<DayPicker {...props} value="2026-03-18" onChange={onChange} />)
    fireEvent.click(screen.getByRole('button', { name: 'One day later' }))
    expect(onChange).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'One day earlier' }))
    expect(onChange).toHaveBeenCalledWith('2026-03-17')
  })
})

describe('Slider', () => {
  it('clamps to its range and respects the step', () => {
    const onChange = vi.fn()
    render(<Slider label="Days late" value={9} min={0} max={10} step={5} onChange={onChange} />)
    const s = screen.getByRole('slider')
    fireEvent.keyDown(s, { key: 'ArrowRight' })
    expect(onChange).toHaveBeenLastCalledWith(10)
    fireEvent.keyDown(s, { key: 'ArrowLeft' })
    expect(onChange).toHaveBeenLastCalledWith(5)
  })
  it('does nothing when disabled', () => {
    const onChange = vi.fn()
    render(<Slider label="Days late" value={3} min={0} max={10} onChange={onChange} disabled />)
    fireEvent.keyDown(screen.getByRole('slider'), { key: 'ArrowRight' })
    expect(onChange).not.toHaveBeenCalled()
  })
})
