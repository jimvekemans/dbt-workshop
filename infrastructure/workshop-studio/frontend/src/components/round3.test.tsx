import { describe, expect, it } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { flowLayout } from './FlowVisual'
import { TasksView, foldTasks, partNumbers } from './TasksView'
import { runChips, runVerdict } from './RunHistoryView'
import { TableView } from './TableView'
import { PipelineView } from './PipelineView'
import { NumberView } from './NumberView'
import { MailView } from './MailView'
import { columnsFor } from '../screens/Home'
import { nextNumberTrails, sampleTrail } from '../api/adapt'
import type { Demo, LiveEvidence, TasksData } from '../api/types'

type Task = TasksData['tasks'][number]
const t = (label: string, state: Task['state'], extra: Partial<Task> = {}): Task => ({ label, state, state_text: state, ...extra })

describe('flowLayout: forks that join again', () => {
  const n = (id: string) => ({ id, label: id, state: 'ok' as const })
  it('puts two alternatives (same source, same target) in one stage', () => {
    const { stages, edges } = flowLayout({ nodes: ['scans', 'day', 'old', 'new', 'report'].map(n), links: [['scans', 'day'], ['day', 'old'], ['day', 'new'], ['old', 'report'], ['new', 'report']] })
    expect(stages.map((s) => s.map((x) => x.id))).toEqual([['scans'], ['day'], ['old', 'new'], ['report']])
    expect(edges).toHaveLength(5)
  })
  it('keeps a plain chain a chain, and never loops on a cycle or an unknown id', () => {
    expect(flowLayout({ nodes: ['a', 'b', 'c'].map(n), links: [['a', 'b'], ['b', 'c']] }).stages).toHaveLength(3)
    expect(flowLayout({ nodes: ['a', 'b'].map(n), links: [['a', 'b'], ['b', 'a'], ['x', 'a']] }).stages.length).toBeGreaterThan(0)
    expect(flowLayout({ nodes: ['a', 'b'].map(n), links: [] }).edges).toEqual([])
  })
  it('lets two changes both lead into the release', () => {
    const { stages, edges } = flowLayout({ nodes: ['pay', 'rev', 'release'].map(n), links: [['pay', 'release'], ['rev', 'release']] })
    expect(stages.map((s) => s.length)).toEqual([2, 1])
    expect(edges.filter((e) => e.to[1] === 0)).toHaveLength(2)
  })
})

describe('tasks: what really happened', () => {
  const failed = [t('Prep', 'done'), t('Build', 'failed'), t('A', 'todo'), t('B', 'todo'), t('C', 'todo'), ...Array.from({ length: 10 }, (_, i) => t(`S${i}`, 'skipped')), t('Sending the failure mail', 'done'), t('Resume', 'done')]
  it('folds by actual state and never hides the failure, the failure mail or the earlier steps', () => {
    render(<TasksView data={{ title: '', tasks: failed }} />)
    expect(screen.getByText('nog 3 stappen niet gerund')).toBeInTheDocument()
    expect(screen.getByText('nog 10 stappen overgeslagen')).toBeInTheDocument()
    expect(screen.queryByText(/stappen klaar/)).toBeNull()
    for (const x of ['Prep', 'Build', 'Sending the failure mail']) expect(screen.getByText(x)).toBeInTheDocument()
  })
  it('says "Not needed" (neutral) for a skipped clean-up in a run without a failure, and numbers repeated parts', () => {
    const ok = [t('Loading one part of the new copy', 'done'), t('Loading one part of the new copy', 'done'), t('Loading one part of the new copy', 'done'), t('Loading one part of the new copy', 'done'), t('Removing the unfinished copy', 'skipped'), t('Sending the failure mail', 'skipped')]
    render(<TasksView data={{ title: '', tasks: ok }} />)
    expect(screen.getAllByText('Niet nodig')).toHaveLength(2)
    expect(screen.queryByText('Overgeslagen')).toBeNull()
    expect(screen.getByText('Deel 1 van 4')).toBeInTheDocument()
    expect(screen.getByText('Deel 4 van 4')).toBeInTheDocument()
  })
  it('uses the backend flags when it sends them', () => {
    expect(partNumbers([t('x', 'done', { part: 2, parts: 5 })]).get(0)).toEqual({ part: 2, parts: 5 })
    render(<TasksView data={{ title: '', tasks: [t('Cleaning', 'skipped', { not_needed: true }), t('Other', 'failed')] }} />)
    expect(screen.getByText('Niet nodig')).toBeInTheDocument()
    expect(foldTasks([t('a', 'skipped', { not_needed: true }), t('b', 'skipped', { not_needed: true }), t('c', 'skipped', { not_needed: true })])[0]).toMatchObject({ kind: 'fold', state: 'not_needed' })
  })
})

describe('run history verdicts and chips', () => {
  const run = { started: '2026-11-15T17:00:00', summary: '' }
  it('"Geen problemen" only with no failure and no warning', () => {
    expect(runVerdict({ ...run, state: 'good' })).toEqual({ state: 'ok', word: 'Geen problemen' })
    expect(runVerdict({ ...run, state: 'good', checks_warned: 1 })).toEqual({ state: 'warn', word: '1 waarschuwing' })
    expect(runVerdict({ ...run, state: 'good', checks_warned: 2 }).word).toBe('2 waarschuwingen')
    expect(runVerdict({ ...run, state: 'good', checks_warned: 2, checks_failed: 1 }).state).toBe('bad')
    expect(runVerdict({ ...run, state: 'bad' }).state).toBe('bad')
  })
  it('repeats the words of the sentence: the whole project counts, left out on purpose is named', () => {
    const chips = runChips({ ...run, state: 'good', tables_built: 25, tables_total: 25, tables_left_out: 3, checks_passed: 14 }).map((c) => c.text)
    expect(chips).toEqual(['25 van 28 tabellen gebuild', '3 bewust overgeslagen', '14 checks geslaagd'])
    expect(runChips({ ...run, state: 'good', retry: true, tables_built: 4, tables_total: 4, tables_left_out: 24, checks_passed: 3 }).map((c) => c.text)).toEqual(['4 tabellen gebuild', '24 bewust overgeslagen', '3 checks geslaagd'])
  })
})

describe('table, pipeline, number, mail', () => {
  it('puts the unit in the header, keeps short values whole and shows the footer', () => {
    const item = { type: 'table', title: 'T', footer: 'Showing the 9 slowest of 28 tables.' } as LiveEvidence
    render(<TableView item={item} title="T" data={{ columns: [{ key: 'k', label: 'Kind' }, { key: 's', label: 'Time', unit: 'seconds' }], rows: [{ cells: { k: 'check', s: 5.1 }, state: null }], empty: '', truncated: false }} />)
    expect(screen.getByRole('columnheader', { name: 'Time (seconds)' })).toBeInTheDocument()
    expect(screen.getByText('check').className).toMatch(/keep/)
    expect(screen.getByText('Showing the 9 slowest of 28 tables.')).toBeInTheDocument()
  })
  it('shows "1 van 28 tabellen in de testrun" as one plain line, with the checks after it', () => {
    render(<PipelineView data={{ state: 'done', state_text: 'Done', jobs: [{ label: 'Try', state: 'done', state_text: 'Done' }], rehearsed: { tables: 1, checks: 3, tables_total: 28, seconds: 2, names: ['the management report'] } }} />)
    expect(screen.getByText('1 van 28 tabellen in de testrun')).toBeInTheDocument()
    expect(screen.getByText('the management report · 3 datachecks · 2 seconden')).toBeInTheDocument()
    expect(screen.getByRole('img', { name: '1 van 28 tabellen' })).toBeInTheDocument()
  })
  it('shows the trail of a number that changed while the step ran', () => {
    render(<NumberView data={{ value: 0, unit: 'rows', compare: null }} trail={[0, 25000, 50000, 100000, 0]} />)
    const p = screen.getByText((_, el) => el?.tagName === 'P' && /Het toonde/.test(el.textContent ?? ''))
    expect(p.textContent!.replace(/\s+daarna\s+/g, ' ').replace(/\s+/g, ' ')).toContain('0 → 25.000 → 50.000 → 100.000 → 0')
  })
  it('opens the mail full size in a dialog that Esc closes', () => {
    render(<MailView data={{ found: true, subject: 'Failed', from: 'a@b', received: null, html: '<p>x</p>' }} />)
    fireEvent.click(screen.getByRole('button', { name: 'Mail op volledige grootte openen' }))
    expect(screen.getByRole('dialog', { name: 'De mail, op volledige grootte' })).toBeInTheDocument()
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(screen.queryByRole('dialog')).toBeNull()
  })
})

describe('number trails and the home grid', () => {
  const demo = (v: number | null): Demo => ({ scenario: 's', status: 'running', steps: [{ id: 'a', status: 'running', actions: [], evidence: [{ type: 'number', data: v === null ? null : { value: v, unit: '', compare: null } } as LiveEvidence] }] })
  it('remembers the values a number showed, drops repeats and starts over when the step runs again', () => {
    let tr = nextNumberTrails({}, demo(0))
    tr = nextNumberTrails(tr, demo(25))
    const same = nextNumberTrails(tr, demo(25))
    expect(same).toBe(tr)
    tr = nextNumberTrails(tr, demo(100))
    expect(tr['s/a/0']).toEqual([0, 25, 100])
    expect(nextNumberTrails(tr, demo(null))).toEqual({})
    expect(nextNumberTrails(tr, null)).toEqual({})
  })
  it('samples a long trail', () => {
    expect(sampleTrail([1, 2, 3])).toEqual([1, 2, 3])
    const s = sampleTrail(Array.from({ length: 13 }, (_, i) => i))
    expect(s[0]).toBe(0)
    expect(s[s.length - 1]).toBe(12)
    expect(s.length).toBeLessThanOrEqual(6)
  })
  it('balances the rows: 3 -> 3, 4 -> 2+2, 5 -> 3+2, 6 -> 3+3, 7 -> 4+3, 8 -> 4+4', () => {
    expect([1, 2, 3, 4, 5, 6, 7, 8].map(columnsFor)).toEqual([1, 2, 3, 2, 3, 3, 4, 4])
  })
})

describe('result comparison lines', () => {
  it('draws object lines as a table and still renders plain strings', async () => {
    const { ResultView } = await import('./ResultView')
    render(<ResultView data={{ summary: 'S', data: { reasons: [{ label: 'dbt', left: '1', right: '1', same: true }, { label: 'x', left: '2', right: '3', same: false }, 'plain line'] } }} />)
    expect(screen.getByRole('row', { name: /dbt/ })).toHaveTextContent('hetzelfde')
    expect(screen.getByText('anders')).toBeInTheDocument()
    expect(screen.getByText('plain line')).toBeInTheDocument()
  })
})
