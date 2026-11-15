import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { RichText, TermProvider } from './Term'

const glossary = { feed: 'A regular delivery of data.', release: 'A numbered bundle of changes.' }

function setup() {
  return render(
    <TermProvider glossary={glossary}>
      <p>
        <RichText text="Our [[feed]] and every [[release]] and an [[unknown]] word." />
      </p>
    </TermProvider>,
  )
}

describe('Term popover', () => {
  it('renders known terms as buttons and unknown ones as plain text', () => {
    setup()
    expect(screen.getAllByRole('button')).toHaveLength(2)
    expect(screen.getByText(/unknown/)).toBeInTheDocument()
  })
  it('opens on click, closes on Escape and returns focus to the term', () => {
    setup()
    const feed = screen.getByRole('button', { name: 'feed' })
    fireEvent.click(feed)
    expect(feed).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByRole('dialog')).toHaveTextContent('A regular delivery of data.')
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(feed).toHaveFocus()
  })
  it('keeps one popover open at a time', () => {
    setup()
    fireEvent.click(screen.getByRole('button', { name: 'feed' }))
    fireEvent.click(screen.getByRole('button', { name: 'release' }))
    expect(screen.getAllByRole('dialog')).toHaveLength(1)
    expect(screen.getByRole('dialog')).toHaveTextContent('numbered bundle')
  })
  it('closes when clicking elsewhere', () => {
    setup()
    fireEvent.click(screen.getByRole('button', { name: 'feed' }))
    fireEvent.mouseDown(document.body)
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})
