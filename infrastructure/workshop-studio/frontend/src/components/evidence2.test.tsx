import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { VersionsView } from './VersionsView'
import { RunHistoryView } from './RunHistoryView'
import { MailView } from './MailView'
import { BarsView } from './BarsView'
import { envWords, envName } from '../lib/env'
import { reducer, initialState } from '../lib/store'

describe('environment names the audience hears', () => {
  it('maps the backend codes', () => {
    expect([envName('dev'), envName('val'), envName('prd')]).toEqual(['development', 'validation', 'production'])
    expect(envWords('Tables built once in prd and val, not dev2')).toBe('Tables built once in production and validation, not dev2')
  })
  it('the versions view prints them, whatever label the backend sends', () => {
    render(<VersionsView data={{ environments: [{ env: 'dev', label: 'LDV', tag: 'v1' }, { env: 'val', label: 'Acceptance', tag: 'v1' }, { env: 'prd', label: 'PRD', tag: 'v0' }] }} />)
    for (const n of ['development', 'validation', 'production']) expect(screen.getByText(n)).toBeInTheDocument()
    expect(screen.queryByText(/LDV|Acceptance|PRD/)).toBeNull()
  })
})

describe('RunHistoryView', () => {
  const local = (d: Date) => new Date(d.getTime() - d.getTimezoneOffset() * 60_000).toISOString().slice(0, 19)
  const now = local(new Date())
  const old = '2026-11-01T02:00:00'
  it('puts the newest night first, marked "Latest", and captions the older ones', () => {
    render(<RunHistoryView data={{ empty: '', runs: [{ started: old, state: 'bad', summary: 'old night' }, { started: now, state: 'good', summary: 'new night' }] }} />)
    expect(screen.getByText('Latest')).toBeInTheDocument()
    expect(screen.getByText('Earlier nights (not part of this demo)')).toBeInTheDocument()
    const items = screen.getAllByRole('listitem')
    expect(items[0]).toHaveTextContent('new night')
    expect(items[1]).toHaveTextContent('old night')
  })
})

describe('MailView', () => {
  it('says so when the message has no text, and takes the iframe out of the tab order', () => {
    const { rerender, container } = render(<MailView data={{ found: true, subject: 'S', from: 'a@b', received: null, html: '  ' }} />)
    expect(screen.getByText('The message has no text.')).toBeInTheDocument()
    rerender(<MailView data={{ found: true, subject: 'S', from: 'a@b', received: new Date().toISOString(), html: '<p>x</p>' }} />)
    expect(container.querySelector('iframe')).toHaveAttribute('tabindex', '-1')
    expect(screen.getByText('a moment ago')).toBeInTheDocument()
  })
})

describe('BarsView draws every slot from the start', () => {
  it('shows dashed outlines for the days that have no data yet', () => {
    const data = { labels: ['2026-11-01', '2026-11-02'], x_label: 'Day', series: [{ name: 'readable', state: 'good' as const, values: [5, 6] }] }
    const { container } = render(<BarsView data={data} title="T" slots={['2026-11-01', '2026-11-02', '2026-11-03', '2026-11-04']} />)
    expect(container.querySelectorAll('rect[stroke-dasharray]')).toHaveLength(2)
    expect(container.textContent).toContain('3 Nov')
  })
})

describe('store: a scenario that does not exist', () => {
  it('is remembered as not found, and forgotten on retry', () => {
    let s = reducer(initialState, { type: 'scenarioFailed', id: 'x', message: 'no', notFound: true })
    expect(s.scenarioNotFound.x).toBe(true)
    s = reducer(s, { type: 'scenarioRetry', id: 'x' })
    expect(s.scenarioNotFound.x).toBeUndefined()
  })
})
