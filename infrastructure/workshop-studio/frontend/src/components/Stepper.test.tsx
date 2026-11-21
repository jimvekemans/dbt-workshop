import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Stepper } from './Stepper'
import { PARTS } from '../lib/router'

describe('Stepper', () => {
  it('shows four numbered, named parts and marks the current one', () => {
    render(<Stepper parts={PARTS} current="idea" onSelect={() => {}} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons).toHaveLength(4)
    expect(screen.getByRole('button', { name: /Het idee/ })).toHaveAttribute('aria-current', 'step')
    expect(screen.getByRole('button', { name: /Wat loopt er mis/ })).not.toHaveAttribute('aria-current')
    expect(screen.getByText('Live demo')).toBeInTheDocument()
  })
  it('selects a part on click', () => {
    const onSelect = vi.fn()
    render(<Stepper parts={PARTS} current="problem" onSelect={onSelect} />)
    fireEvent.click(screen.getByRole('button', { name: /De code/ }))
    expect(onSelect).toHaveBeenCalledWith('code')
  })
})
