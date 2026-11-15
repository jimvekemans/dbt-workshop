import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { BarsView } from './BarsView'
import { PipelineView } from './PipelineView'
import { TasksView } from './TasksView'
import { RunHistoryView } from './RunHistoryView'
import { highlightLines } from '../lib/highlight'
import { barsModel } from '../api/adapt'
import { plainText } from '../lib/glossary'

describe('bars with `bars`', () => {
  const data = { labels: ['x'], x_label: 'Day', bars: [{ key: 'a', label: '14 Nov' }, { key: 'b', label: '15 Nov (today)', today: true }], series: [{ name: 'Parcels', state: 'good' as const, values: [5, 2] }] }
  it('uses the order and labels of `bars`, marks today and says the unit', () => {
    expect(barsModel(data).labels).toEqual(['14 Nov', '15 Nov (today)'])
    render(<BarsView data={data} title="T" unit="parcel scans" />)
    expect(screen.getByText('today')).toBeInTheDocument()
    expect(screen.getByText('Each bar counts parcel scans.')).toBeInTheDocument()
  })
  it('still works without `bars`', () => {
    render(<BarsView data={{ labels: ['a', 'b'], x_label: '', series: [{ name: 'S', state: 'neutral', values: [1, 2] }] }} title="T" />)
    expect(screen.queryByText('today')).toBeNull()
  })
})

describe('pipeline failures', () => {
  it('shows the failure under the failed job and a manual job as waiting for a person', () => {
    render(<PipelineView data={{ state: 'failed', state_text: 'Stopped', jobs: [{ label: 'Run checks', state: 'failed', state_text: 'Failed' }, { label: 'Release', state: 'manual', state_text: '' }], failures: [{ kind: 'check', text: 'No parcel without address', rows: 12, details: 'secret sql' }] }} />)
    expect(screen.getByText(/What failed:/)).toBeInTheDocument()
    expect(screen.getByText(/12 rows/)).toBeInTheDocument()
    expect(screen.getByText('Waiting for a person')).toBeInTheDocument()
    expect(screen.queryByText('secret sql')).toBeNull()
  })
})

describe('tasks', () => {
  const t = (label: string, state: 'done' | 'failed' | 'skipped', extra = {}) => ({ label, state, state_text: state, ...extra })
  it('folds three or more quiet rows after the first failure, with a keyboard toggle', () => {
    render(<TasksView data={{ title: '', tasks: [t('A', 'done', { count: 5 }), t('B', 'failed'), t('C1', 'skipped'), t('C2', 'skipped'), t('C3', 'skipped')] }} />)
    expect(screen.getByText('×5')).toBeInTheDocument()
    expect(screen.getByText('3 more steps skipped')).toBeInTheDocument()
    expect(screen.queryByText('C1')).toBeNull()
    const b = screen.getByRole('button', { name: 'Show all' })
    expect(b).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(b)
    expect(screen.getByText('C1')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Hide' })).toHaveAttribute('aria-expanded', 'true')
  })
  it('never folds before a failure or fewer than three', () => {
    render(<TasksView data={{ title: '', tasks: [t('A', 'skipped'), t('B', 'skipped'), t('C', 'skipped'), t('D', 'failed'), t('E', 'skipped'), t('F', 'skipped')] }} />)
    expect(screen.queryByText(/more steps/)).toBeNull()
  })
})

describe('run history chips', () => {
  it('shows the numbers that exist', () => {
    render(<RunHistoryView data={{ empty: '', runs: [{ started: '2026-11-15T02:00:00', state: 'bad', summary: 'Ended early', current: true, tables_built: 24, tables_total: 28, tables_not_run: 4, checks_failed: 1, checks_not_run: 2 }] }} />)
    for (const x of ['24 of 28 tables built', '4 tables not run', '1 check failed', '2 checks not run', 'This run', 'Problems']) expect(screen.getByText(x)).toBeInTheDocument()
  })
  it('says "1 warning" (not "No problems") for a night with only a warning, and "Retry" for a retry', () => {
    render(<RunHistoryView data={{ empty: '', runs: [{ started: '2026-11-15T02:00:00', state: 'good', summary: 'Warned', tables_built: 28, tables_total: 28, checks_passed: 20, checks_warned: 1 }, { started: '2026-11-15T03:00:00', state: 'good', summary: 'Retry: 4 tables built', retry: true, current: true, tables_built: 4, tables_total: 4, tables_left_out: 24 }] }} />)
    expect(screen.getByText('1 warning', { selector: 'li' })).toBeInTheDocument()
    expect(screen.getAllByText('1 warning')).toHaveLength(2)
    expect(screen.queryByText('Problems')).toBeNull()
    expect(screen.getByText('Retry')).toBeInTheDocument()
    expect(screen.getByText('4 tables built')).toBeInTheDocument()
    expect(screen.getByText('28 of 28 tables built')).toBeInTheDocument()
  })
})

describe('mask and glossary', () => {
  it('keeps the mask readable and titled', () => {
    expect(highlightLines(["password: '••••••'"], 'yaml')[0]).toContain('title="Hidden on purpose">••••••</span>')
  })
  it('plainText never keeps brackets', () => {
    expect(plainText('A [[feed]] and [[a|b]]')).toBe('A feed and b')
  })
})
