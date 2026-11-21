import { beforeEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { TextSize } from './TextSize'
import { DEFAULT_TEXT_SIZE, TEXT_SIZES } from '../lib/textSize'

const scale = () => document.documentElement.style.getPropertyValue('--text-scale')

describe('TextSize', () => {
  beforeEach(() => {
    localStorage.clear()
    document.documentElement.style.removeProperty('--text-scale')
  })

  it('starts at Normaal (0.65) and steps up through the sizes, remembering the choice', () => {
    render(<TextSize />)
    expect(screen.getByRole('group', { name: 'Tekstgrootte' })).toBeInTheDocument()
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Normaal')
    expect(scale()).toBe('0.65')
    fireEvent.click(screen.getByRole('button', { name: 'Grotere tekst' }))
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Groot')
    expect(scale()).toBe('0.75')
    expect(localStorage.getItem('studio-text-size-v2')).toBe('large')
    fireEvent.click(screen.getByRole('button', { name: 'Grotere tekst' }))
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Extra groot')
    expect(scale()).toBe('0.85')
    // the largest: the button says why it does nothing more
    const larger = screen.getByRole('button', { name: 'Grotere tekst' })
    expect(larger).toHaveAttribute('aria-disabled', 'true')
    expect(larger).toHaveAttribute('title', 'Dit is de grootste tekstgrootte')
    fireEvent.click(larger)
    expect(scale()).toBe('0.85')
  })

  it('reads a stored choice at the start, goes down to Extra klein (0.45) and stops there', () => {
    localStorage.setItem('studio-text-size-v2', 'small')
    render(<TextSize />)
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Klein')
    expect(scale()).toBe('0.55')
    fireEvent.click(screen.getByRole('button', { name: 'Kleinere tekst' }))
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Extra klein')
    expect(scale()).toBe('0.45')
    const smaller = screen.getByRole('button', { name: 'Kleinere tekst' })
    expect(smaller).toHaveAttribute('aria-disabled', 'true')
    expect(smaller).toHaveAttribute('title', 'Dit is de kleinste tekstgrootte')
  })

  it('has exactly five sizes from 0.45 to 0.85, with the default in the middle', () => {
    expect(TEXT_SIZES.map((s) => s.scale)).toEqual([0.45, 0.55, 0.65, 0.75, 0.85])
    expect(TEXT_SIZES.map((s) => s.label)).toEqual(['Extra klein', 'Klein', 'Normaal', 'Groot', 'Extra groot'])
    expect(TEXT_SIZES.find((s) => s.id === DEFAULT_TEXT_SIZE)?.scale).toBe(0.65)
  })

  it('ignores a stored value it does not know, and the key of the old table', () => {
    localStorage.setItem('studio-text-size-v2', 'huge')
    render(<TextSize />)
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Normaal')
    cleanup()
    // an old choice ("xlarge" was 1.15 in the old table) lives under the old key and must not carry over
    localStorage.clear()
    localStorage.setItem('studio-text-size', 'xlarge')
    render(<TextSize />)
    expect(screen.getByRole('status')).toHaveTextContent('Tekstgrootte: Normaal')
    expect(scale()).toBe('0.65')
  })
})
