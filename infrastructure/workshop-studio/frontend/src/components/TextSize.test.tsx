import { beforeEach, describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { TextSize } from './TextSize'

const scale = () => document.documentElement.style.getPropertyValue('--text-scale')

describe('TextSize', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--text-scale')
  })

  it('starts at Medium and steps up through the sizes, remembering the choice', () => {
    render(<TextSize />)
    expect(screen.getByRole('group', { name: 'Text size' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Text size: Medium')
    expect(scale()).toBe('0.85')
    fireEvent.click(screen.getByRole('button', { name: 'Larger text' }))
    expect(screen.getByRole('status')).toHaveTextContent('Text size: Large')
    expect(scale()).toBe('1')
    expect(localStorage.getItem('studio-text-size')).toBe('large')
    fireEvent.click(screen.getByRole('button', { name: 'Larger text' }))
    expect(scale()).toBe('1.15')
    // the largest: the button says why it does nothing more
    const larger = screen.getByRole('button', { name: 'Larger text' })
    expect(larger).toHaveAttribute('aria-disabled', 'true')
    expect(larger).toHaveAttribute('title', 'This is the largest text size')
    fireEvent.click(larger)
    expect(scale()).toBe('1.15')
  })

  it('reads a stored choice at the start, goes down to Extra small and stops there', () => {
    localStorage.setItem('studio-text-size', 'small')
    render(<TextSize />)
    expect(screen.getByRole('status')).toHaveTextContent('Text size: Small')
    expect(scale()).toBe('0.75')
    fireEvent.click(screen.getByRole('button', { name: 'Smaller text' }))
    expect(screen.getByRole('status')).toHaveTextContent('Text size: Extra small')
    expect(scale()).toBe('0.65')
    expect(screen.getByRole('button', { name: 'Smaller text' })).toHaveAttribute('aria-disabled', 'true')
  })

  it('ignores a stored value it does not know', () => {
    localStorage.setItem('studio-text-size', 'huge')
    render(<TextSize />)
    expect(screen.getByRole('status')).toHaveTextContent('Text size: Medium')
  })
})
