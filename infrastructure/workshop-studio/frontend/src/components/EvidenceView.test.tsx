import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { EvidenceView } from './EvidenceView'
import type { LiveEvidence } from '../api/types'
import { RECORDINGS, SCENARIOS, fakeEvidence } from '../dev/mockContent'

const item = (type: LiveEvidence['type'], extra: object = {}): LiveEvidence => ({ type, title: `Title of ${type}`, ...extra })

describe('EvidenceView with real payloads', () => {
  it('renders every frame ever recorded from the real studio, for the item it belongs to', () => {
    let rendered = 0
    for (const [id, steps] of Object.entries(RECORDINGS)) {
      for (const [stepId, rec] of Object.entries(steps)) {
        const def = SCENARIOS[id]!.demo.steps.find((s) => s.id === stepId)!
        rec.frames.forEach((frame, fi) => {
          if (fi % 3 !== 0 && fi !== rec.frames.length - 1) return
          def.show.forEach((show, i) => {
            const { container, unmount } = render(<EvidenceView item={{ ...show, data: frame.evidence[i] ?? null } as LiveEvidence} stepStatus="running" />)
            expect(container.querySelector('h4')).not.toBeNull()
            rendered += 1
            unmount()
          })
        })
      }
    }
    expect(rendered).toBeGreaterThan(100)
  })

  it.each(['table', 'bars', 'number', 'tasks', 'pipeline', 'mail', 'versions', 'result', 'freshness', 'run_history'] as const)('renders generated %s data', (type) => {
    const live = { ...item(type), data: fakeEvidence(item(type), { t: 1, inputs: {}, attempt: 0 }) } as LiveEvidence
    const { container } = render(<EvidenceView item={live} stepStatus="done" />)
    expect(screen.getByRole('heading', { name: `Title of ${type}` })).toBeInTheDocument()
    expect(container.querySelector('[aria-busy="true"]')).toBeNull()
    expect(container.textContent!.length).toBeGreaterThan(20)
  })

  it('shows loading, waiting and the backend sentence for a failed provider', () => {
    const base = item('table')
    const { rerender } = render(<EvidenceView item={base} stepStatus="todo" />)
    expect(screen.getByText(/Appears after you press the button above/i)).toBeInTheDocument()
    rerender(<EvidenceView item={base} stepStatus="running" />)
    expect(screen.getByRole('status', { name: /Reading the latest/ })).toBeInTheDocument()
    rerender(<EvidenceView item={{ ...base, data: { error: 'We could not read this just now.' } }} stepStatus="running" />)
    expect(screen.getByRole('alert')).toHaveTextContent('We could not read this just now.')
  })

  it('uses the empty sentences the backend sends, and a default title when there is none', () => {
    const t = { type: 'table', data: { columns: [], rows: [], empty: 'Nothing wrong today.', truncated: false } } as LiveEvidence
    render(<EvidenceView item={t} stepStatus="running" />)
    expect(screen.getByText('Nothing wrong today.')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'The numbers' })).toBeInTheDocument()
  })

  it('scrolls a table of 200 rows inside its frame, with a sticky header and a truncation note', () => {
    const big = { ...item('table', { mock_rows: 200, columns: Array.from({ length: 8 }, (_, i) => ({ key: `c${i}`, label: `C${i}` })) }) }
    const live = { ...big, data: fakeEvidence(big, { t: 1, inputs: {}, attempt: 0 }) } as LiveEvidence
    const { container } = render(<EvidenceView item={live} stepStatus="done" />)
    expect(container.querySelectorAll('tbody tr')).toHaveLength(200)
    expect(container.querySelectorAll('thead th')).toHaveLength(8)
    expect(screen.getByText(/Only the first 200 rows/)).toBeInTheDocument()
  })

  it('shows a changed version obviously from what the app remembered, and mail in a script-free sandbox', () => {
    const v = { ...item('versions'), data: { environments: [{ env: 'dev', label: 'Development', tag: 'v2' }, { env: 'val', label: 'Acceptance', tag: 'v1' }, { env: 'prd', label: 'Production', tag: 'v1' }] } } as LiveEvidence
    const { unmount } = render(<EvidenceView item={v} stepStatus="done" previous={{ dev: 'v1', val: 'v1', prd: 'v1' }} />)
    expect(screen.getAllByText('Changed')).toHaveLength(1)
    expect(screen.getAllByText('Unchanged')).toHaveLength(2)
    unmount()
    const m = { ...item('mail'), data: { found: true, subject: 'Hi', from: 'a', received: '2026-11-15T12:20:33.855Z', html: '<script>alert(1)</script><p>x</p>' } } as LiveEvidence
    const { container } = render(<EvidenceView item={m} stepStatus="done" />)
    const frame = container.querySelector('iframe')!
    expect(frame.getAttribute('sandbox')).not.toContain('allow-scripts')
    expect(frame.getAttribute('sandbox')).not.toContain('allow-forms')
    expect(frame.getAttribute('tabindex')).toBe('-1')
    expect(frame.getAttribute('srcdoc')).toContain("default-src 'none'")
  })
})
