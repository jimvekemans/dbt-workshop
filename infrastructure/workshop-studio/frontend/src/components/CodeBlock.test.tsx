import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { CodeBlock } from './CodeBlock'
import { segments } from '../lib/code'
import type { CodeBlockData, CodeLine } from '../api/types'

const lines = (n: number, noted: Record<number, string>): CodeLine[] =>
  Array.from({ length: n }, (_, i) => ({ n: i + 1, text: `line ${i + 1}`, ...(noted[i] ? { note: noted[i] } : {}) }))
const block = (over: Partial<CodeBlockData> = {}): CodeBlockData => ({
  title: 'T', file_label: 'The recipe for the table of parcel scans', file: 'models/edw/stg_carrier_events.sql', language: 'sql', gitlab_url: 'http://gitlab.local/x', lines: lines(12, { 1: 'First note about [[feed]] words', 10: 'Second note' }), ...over,
})

describe('segments (folding runs of lines without a note)', () => {
  it('folds a run of 4 or more and keeps shorter runs', () => {
    const s = segments(lines(10, { 0: 'a', 4: 'b', 9: 'c' }), new Set())
    expect(s).toEqual([{ kind: 'line', i: 0 }, { kind: 'line', i: 1 }, { kind: 'line', i: 2 }, { kind: 'line', i: 3 }, { kind: 'line', i: 4 }, { kind: 'gap', start: 5, end: 9 }, { kind: 'line', i: 9 }])
  })
  it('never folds a block that has no notes at all, and opens a run on request (with a way to close it again)', () => {
    expect(segments(lines(8, {}), new Set()).every((x) => x.kind === 'line')).toBe(true)
    const open = segments(lines(10, { 0: 'a', 9: 'b' }), new Set([1]))
    expect(open.filter((x) => x.kind === 'line')).toHaveLength(10)
    expect(open.filter((x) => x.kind === 'hide')).toEqual([{ kind: 'hide', start: 1, end: 9 }])
  })
})

describe('CodeBlock', () => {
  it('shows the plain label as heading, the path below it, and one short GitLab link', () => {
    render(<CodeBlock block={block()} />)
    expect(screen.getByText('The recipe for the table of parcel scans')).toBeInTheDocument()
    expect(screen.getByText('models/edw/stg_carrier_events.sql').tagName).toBe('CODE')
    expect(screen.getAllByRole('link', { name: /Openen in GitLab/ })).toHaveLength(1)
    expect(screen.queryByText('Needs the workshop login.')).toBeNull()
  })
  it('shows no link when there is no URL', () => {
    render(<CodeBlock block={block({ gitlab_url: '' })} />)
    expect(screen.queryByRole('link')).toBeNull()
  })
  it('shows every note once, right under its line, with its number badge, and no list above the code', () => {
    const { container } = render(<CodeBlock block={block()} />)
    expect(screen.queryByRole('list', { name: /Notes for this code/ })).toBeNull()
    expect(container.querySelectorAll('[role="note"]')).toHaveLength(2)
    expect(container.querySelector('[data-note="1"] [role="note"]')).toHaveTextContent('First note about feed words')
    expect(container.querySelector('[data-note="2"] [role="note"]')).toHaveTextContent('Second note')
    // the line points at its note
    expect(container.querySelector('[data-note="1"] code')!.getAttribute('aria-describedby')).toBe(container.querySelector('[data-note="1"] [role="note"]')!.id)
  })
  it('lets the code scroll as a keyboard-focusable group', () => {
    render(<CodeBlock block={block()} />)
    expect(screen.getByRole('group', { name: /Code uit The recipe/ })).toHaveAttribute('tabindex', '0')
  })
  it('folds the run of 8 plain lines behind "Toon regels 3–10 (nog 8)", opens it, and can hide it again', () => {
    const { container } = render(<CodeBlock block={block()} />)
    expect(container.textContent).not.toContain('line 5')
    fireEvent.click(screen.getByRole('button', { name: 'Toon regels 3–10 (nog 8)' }))
    expect(container.textContent).toContain('line 5')
    expect(screen.queryByRole('button', { name: /Toon regels/ })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Verberg regels 3–10' }))
    expect(container.textContent).not.toContain('line 5')
  })
  it('lights a line and its note together', () => {
    const { container } = render(<CodeBlock block={block()} />)
    fireEvent.focus(container.querySelector('[data-note="2"] code')!)
    expect(container.querySelector('[data-note="2"]')).toHaveAttribute('data-active')
    expect(container.querySelector('[data-note="1"]')).not.toHaveAttribute('data-active')
    fireEvent.mouseEnter(container.querySelector('[data-note="1"] [role="note"]')!)
    expect(container.querySelector('[data-note="1"]')).toHaveAttribute('data-active')
  })
})
