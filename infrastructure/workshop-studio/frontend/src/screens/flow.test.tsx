import { describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { StatusBadge } from '../components/StatusBadge'
import { ScenarioCard, flowLabel } from '../components/ScenarioCard'
import { CodeBlock } from '../components/CodeBlock'
import { StepCard } from './StepCard'
import { DemoPart } from './DemoPart'
import { CodePart } from './CodePart'
import { baseServer, mount } from '../test/harness'
import { CATALOG, SCENARIOS } from '../dev/mockContent'
import { FLOW, withFlowAndRoadmap } from '../dev/mockFlow'
import type { CatalogScenario, Scenario, ServerState } from '../api/types'

/** The sample flow stretched to the largest size a flow may have: 8 steps and 8 code blocks. */
const eight: Scenario = {
  ...structuredClone(FLOW),
  demo: {
    ...structuredClone(FLOW.demo),
    steps: [...structuredClone(FLOW.demo.steps), ...['seven', 'eight'].map((id) => ({ ...structuredClone(FLOW.demo.steps[0]!), id, title: `Extra stage ${id}`, feature: `Extra ${id}`, status: 'next' as const }))],
  },
  code: [...structuredClone(FLOW.code), ...structuredClone(FLOW.code.slice(0, 3)).map((b, i) => ({ ...b, title: `Extra block ${i + 6}` }))],
}

const open = (sc: Scenario, done: string[] = []): ServerState => ({
  ...baseServer,
  demo: {
    scenario: sc.id,
    status: 'ready',
    steps: sc.demo.steps.map((s) => ({ id: s.id, status: done.includes(s.id) ? ('done' as const) : ('todo' as const), actions: [], evidence: s.show.map((e) => ({ ...e, data: null })) })),
  },
})

describe('StatusBadge', () => {
  it('says "Vandaag al in gebruik" or "Volgende stap", and nothing without a status', () => {
    const today = render(<StatusBadge status="today" />)
    expect(screen.getByText('Vandaag al in gebruik')).toBeInTheDocument()
    expect(today.container.querySelector('[data-status="today"]')).not.toBeNull()
    today.unmount()
    const next = render(<StatusBadge status="next" />)
    expect(screen.getByText('Volgende stap')).toBeInTheDocument()
    expect(next.container.querySelector('[data-status="next"]')).not.toBeNull()
    next.unmount()
    for (const status of [null, undefined, 'whenever']) {
      const none = render(<StatusBadge status={status} />)
      expect(none.container.querySelector('[data-status]')).toBeNull()
      none.unmount()
    }
  })

  it('is on a step card with its feature, and absent on a step without a status', () => {
    const def = FLOW.demo.steps[3]!
    const card = (d: typeof def, index: number) => <StepCard scenarioId={FLOW.id} index={index} total={6} def={d} state={null} blocked={null} values={{}} onValue={() => undefined} onRun={() => undefined} />
    const first = mount(card(def, 3), null)
    expect(screen.getByText('Volgende stap')).toBeInTheDocument()
    expect(screen.getByText('Private test copy')).toBeInTheDocument()
    first.unmount()
    const { container } = mount(card(FLOW.demo.steps[5]!, 5), null)
    expect(container.querySelector('[data-status="today"], [data-status="next"]')).toBeNull()
    expect(screen.queryByText('Volgende stap')).toBeNull()
    expect(screen.queryByText('Vandaag al in gebruik')).toBeNull()
  })

  it('is on a code block when it has a status, and not otherwise', () => {
    const withBadge = render(<CodeBlock block={FLOW.code[3]!} />)
    expect(screen.getByText('Volgende stap')).toBeInTheDocument()
    withBadge.unmount()
    const without = render(<CodeBlock block={SCENARIOS['switchover-date']!.code[0]!} />)
    expect(without.container.querySelector('[data-status]')).toBeNull()
  })
})

describe('the flow card on the home page', () => {
  const entry: CatalogScenario = withFlowAndRoadmap(CATALOG, () => true).groups.at(-1)!.scenarios[0]!
  it('is marked "Flow · 6 stappen"', () => {
    expect(flowLabel(6)).toBe('Flow · 6 stappen')
    expect(flowLabel(1)).toBe('Flow · 1 stap')
    mount(<ScenarioCard s={entry} number={21} />, null)
    expect(screen.getByText('Flow · 6 stappen')).toBeInTheDocument()
  })
  it('is not drawn on an ordinary scenario, which shows its status badge instead', () => {
    const plain = CATALOG.groups[0]!.scenarios[0]!
    const first = mount(<ScenarioCard s={plain} number={1} />, null)
    expect(screen.queryByText(/^Flow/)).toBeNull()
    expect(screen.queryByText('Vandaag al in gebruik')).toBeNull()
    first.unmount()
    mount(<ScenarioCard s={{ ...plain, feature: 'Late feeds', status: 'today' }} number={1} />, null)
    expect(screen.getByText('Vandaag al in gebruik')).toBeInTheDocument()
  })
})

describe('a demo with 8 steps', () => {
  it('shows a map of the steps and every step card, each with its own badge', () => {
    mount(<DemoPart scenario={eight} />, open(eight, ['pick_up', 'check']))
    return screen.findByRole('navigation', { name: 'De stappen van deze demo' }).then((nav) => {
      const jumps = within(nav).getAllByRole('button')
      expect(jumps).toHaveLength(8)
      expect(jumps[0]).toHaveTextContent('✓')                       // done
      expect(jumps[2]).toHaveTextContent('3')                       // not yet
      expect(screen.getAllByRole('article')).toHaveLength(8)
      expect(screen.getAllByText('Volgende stap')).toHaveLength(4)                         // steps 4, 5, 7, 8
      expect(screen.getAllByText('Vandaag al in gebruik')).toHaveLength(3)                // steps 1, 2, 3
    })
  })

  it('jumps to a step when its button is pressed', async () => {
    mount(<DemoPart scenario={eight} />, open(eight))
    const scroll = vi.fn()
    Element.prototype.scrollIntoView = scroll
    const nav = await screen.findByRole('navigation', { name: 'De stappen van deze demo' })
    fireEvent.click(within(nav).getAllByRole('button')[6]!)
    expect(scroll).toHaveBeenCalled()
    expect(document.activeElement?.id).toBe('step-seven')
  })

  it('needs no map for four steps or fewer', async () => {
    const four = SCENARIOS['switchover-date']!
    mount(<DemoPart scenario={four} />, open(four))
    await screen.findAllByRole('article')
    expect(screen.queryByRole('navigation', { name: 'De stappen van deze demo' })).toBeNull()
  })
})

describe('a code part with 8 blocks', () => {
  it('lists every block, with its number, and badges only on the blocks that have one', () => {
    const { container } = mount(<CodePart scenario={eight} />, null)
    const nav = screen.getByRole('navigation', { name: 'De codeblokken van dit scenario' })
    expect(within(nav).getAllByRole('button')).toHaveLength(8)
    expect(container.querySelectorAll('figure')).toHaveLength(8)
    expect(screen.getByText('Blok 8 van 8')).toBeInTheDocument()
    expect(container.querySelectorAll('[data-status="today"]')).toHaveLength(6)   // blocks 1-3 and the three repeats
    expect(container.querySelectorAll('[data-status="next"]')).toHaveLength(2)
  })
  it('has no map for three blocks or fewer', () => {
    mount(<CodePart scenario={SCENARIOS['switchover-date']!} />, null)
    expect(screen.queryByRole('navigation', { name: 'De codeblokken van dit scenario' })).toBeNull()
  })
})
