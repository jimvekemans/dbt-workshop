import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { FlowVisual } from './FlowVisual'
import { TimelineVisual } from './TimelineVisual'
import { CompareVisual } from './CompareVisual'
import { groupDays, visualTone } from '../lib/visual'
import type { FlowVisual as Flow, TimelineDay, TimelineVisual as Timeline, CompareVisual as Compare } from '../api/types'

const day = (label: string, state: TimelineDay['state'], note: string): TimelineDay => ({ label, state, note })
const days = [day('Mon', 'ok', '1,200 delivered'), day('Tue', 'ok', '1,100 delivered'), day('Wed', 'bad', '0 delivered'), day('Thu', 'bad', '0 delivered'), day('Fri', 'bad', '0 delivered'), day('Sat', 'warn', '0 delivered')]

describe('groupDays (phone: equal neighbouring days become one row)', () => {
  it('merges neighbours with the same state and note into "Wed tot Fri"', () => {
    const g = groupDays(days)
    expect(g.map((x) => x.label)).toEqual(['Mon', 'Tue', 'Wed tot Fri', 'Sat'])
    expect(g[2]).toMatchObject({ from: 2, to: 4, state: 'bad', note: '0 delivered' })
  })
  it('never merges across the marker', () => {
    expect(groupDays(days, 3).map((x) => x.label)).toEqual(['Mon', 'Tue', 'Wed tot Thu', 'Fri', 'Sat'])
  })
  it('treats an unknown state as idle', () => {
    expect(visualTone('weird')).toBe('idle')
    expect(visualTone('fix')).toBe('fix')
  })
})

describe('TimelineVisual', () => {
  const visual: Timeline = { type: 'timeline', days, marker: { after: 1, label: 'The format changes' }, caption: 'cap' }
  it('draws one column per day and one row per group, with the marker in both, and numbers per day', () => {
    const { container } = render(<TimelineVisual visual={visual} />)
    expect(container.querySelectorAll('ol')).toHaveLength(2)
    expect(container.querySelectorAll('ol')[0]!.querySelectorAll('li')).toHaveLength(6)
    expect(screen.getAllByText('The format changes')).toHaveLength(2)
    expect(container.querySelectorAll('[role="note"]')).toHaveLength(2)
    expect(screen.getAllByText('1,200 delivered')).toHaveLength(2)
    expect(screen.getByText('Wed tot Fri')).toBeInTheDocument()
  })
  it('says nothing for a fine day but names a problem', () => {
    render(<TimelineVisual visual={visual} />)
    expect(screen.queryByText('In orde')).toBeNull()
    expect(screen.getAllByText('Probleem').length).toBeGreaterThan(0)
  })
})

describe('FlowVisual', () => {
  const nodes = [
    { id: 'a', label: 'Source', state: 'ok' as const },
    { id: 'b', label: 'Check', state: 'fix' as const, note: 'Stops a bad file' },
    { id: 'c', label: 'Report', state: 'bad' as const },
    { id: 'd', label: 'Mystery', state: 'sparkly' as unknown as 'ok' },
  ]
  const visual: Flow = { type: 'flow', nodes, links: [['a', 'b'], ['b', 'c'], ['c', 'd']] }
  it('marks the fix with the words "De oplossing", a bad box with a status, and an ok box with nothing', () => {
    const { container } = render(<FlowVisual visual={visual} />)
    expect(screen.getByText('De oplossing')).toBeInTheDocument()
    expect(screen.getByText('Probleem')).toBeInTheDocument()
    expect(screen.queryByText('In orde')).toBeNull()
    const nodeEls = container.querySelectorAll('[data-state]')
    expect([...nodeEls].map((e) => e.getAttribute('data-state'))).toEqual(['ok', 'fix', 'bad', 'idle'])
  })
})

describe('CompareVisual', () => {
  const visual: Compare = {
    type: 'compare',
    left: { title: 'Nothing checks', items: ['a'], state: 'bad' },
    right: { title: 'A check runs', items: ['b'], state: 'ok' },
  }
  it('labels the sides "Zonder vangnet" and "Met vangnet"; the good side says Beter', () => {
    render(<CompareVisual visual={visual} />)
    expect(screen.getByText('Zonder vangnet')).toBeInTheDocument()
    expect(screen.getByText('Met vangnet')).toBeInTheDocument()
    expect(screen.getByText('Beter')).toBeInTheDocument()
    expect(screen.getByText('Probleem')).toBeInTheDocument()
    expect(screen.queryByText('In orde')).toBeNull()
  })
})
