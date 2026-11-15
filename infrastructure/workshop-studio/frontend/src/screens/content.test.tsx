import { describe, expect, it } from 'vitest'
import { render } from '@testing-library/react'
import { TermProvider } from '../components/Term'
import { ProblemPart } from './ProblemPart'
import { IdeaPart } from './IdeaPart'
import { CodePart } from './CodePart'
import { CATALOG, SCENARIOS } from '../dev/mockContent'
import { parseGlossary, lookupTerm } from '../lib/glossary'
import { StepCard } from './StepCard'
import { StudioProvider } from '../lib/studio'
import { ToastProvider } from '../components/Toast'
import { defaultsFor } from '../lib/inputs'
import type { Backend } from '../api/types'

const ids = Object.keys(SCENARIOS)
const stub = { getCatalog: () => new Promise(() => undefined), getState: () => new Promise(() => undefined), subscribe: () => () => undefined } as unknown as Backend

const texts = (v: unknown): string[] => (typeof v === 'string' ? [v] : Array.isArray(v) ? v.flatMap(texts) : v && typeof v === 'object' ? Object.values(v).flatMap(texts) : [])

describe('the real scenarios and the two test scenarios', () => {
  it('has the real catalog (every one of its scenarios has content) and the two mock-only test scenarios', () => {
    const real = CATALOG.groups.flatMap((g) => g.scenarios)
    expect(CATALOG.groups.length).toBeGreaterThan(0)
    for (const s of real) expect(ids).toContain(s.id)
    expect(ids).toEqual(expect.arrayContaining(['stress-test', 'states-test']))
    expect(ids).toHaveLength(real.length + 2)
  })

  it.each(ids)('%s: every glossary term resolves and parts 1 to 3 render', (id) => {
    const sc = SCENARIOS[id]!
    // code lines are code (bash has [[ ... ]]); every other text is prose
    const prose = { ...sc, code: sc.code.map((c) => ({ ...c, lines: c.lines.map((l) => l.note) })) }
    for (const text of texts(prose)) for (const seg of parseGlossary(text)) if (seg.kind === 'term') expect(lookupTerm(CATALOG.glossary, seg.term), `[[${seg.term}]] in ${id}`).toBeTruthy()
    const { container } = render(
      <ToastProvider>
        <StudioProvider backend={stub}>
          <TermProvider glossary={CATALOG.glossary}>
            <ProblemPart scenario={sc} />
            <IdeaPart scenario={sc} />
            <CodePart scenario={sc} />
          </TermProvider>
        </StudioProvider>
      </ToastProvider>,
    )
    expect(container.textContent).toContain(sc.problem.headline.slice(0, 20))
    const links = container.querySelectorAll('a[href^="http://gitlab"]').length
    expect(links).toBe(sc.code.filter((c) => c.gitlab_url).length)
  })

  it.each(ids)('%s: every step card renders with its inputs and evidence placeholders', (id) => {
    const sc = SCENARIOS[id]!
    sc.demo.steps.forEach((def, i) => {
      const { container, unmount } = render(
        <ToastProvider>
          <StudioProvider backend={stub}>
            <TermProvider glossary={CATALOG.glossary}>
              <StepCard scenarioId={id} index={i} total={sc.demo.steps.length} def={def} state={null} blocked={null} values={defaultsFor(def.inputs)} onValue={() => undefined} onRun={() => undefined} />
            </TermProvider>
          </StudioProvider>
        </ToastProvider>,
      )
      expect(container.querySelectorAll('section h4')).toHaveLength(def.show.length)
      for (const inp of def.inputs) expect(container.textContent).toContain(inp.label)
      expect(container.querySelectorAll('button').length).toBeGreaterThanOrEqual(1)
      unmount()
    })
  })
})
