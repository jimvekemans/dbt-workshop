import { beforeEach, describe, expect, it } from 'vitest'
import { fireEvent, screen } from '@testing-library/react'
import { ScenarioCard } from '../components/ScenarioCard'
import { TechNamesToggle } from '../components/TechNamesToggle'
import { TechLine } from '../components/TechLine'
import { TopBar } from '../components/TopBar'
import { Switcher } from '../components/Switcher'
import { StepCard } from './StepCard'
import { ScenarioPage } from './ScenarioPage'
import { Roadmap } from './Roadmap'
import { mount } from '../test/harness'
import { searchScenarios } from '../lib/store'
import { techText, TECH_KEY } from '../lib/techNames'
import { CATALOG, SCENARIOS } from '../dev/mockContent'
import { FLOW, withFlowAndRoadmap } from '../dev/mockFlow'
import type { Catalog, CatalogScenario, PartId } from '../api/types'

const full: Catalog = withFlowAndRoadmap(CATALOG, () => true)
const entry = (id: string): CatalogScenario => full.groups.flatMap((g) => g.scenarios).find((s) => s.id === id)!
const lines = (c: HTMLElement) => Array.from(c.querySelectorAll('[data-tech]')).map((e) => e.textContent)

beforeEach(() => localStorage.clear())

describe('the switch "Technische namen"', () => {
  it('is on at the start, can be switched off and on, and is remembered in this browser', () => {
    mount(<TechNamesToggle />, null)
    const sw = screen.getByRole('switch', { name: 'Technische namen' })
    expect(sw).toHaveAttribute('aria-checked', 'true')
    fireEvent.click(sw)
    expect(sw).toHaveAttribute('aria-checked', 'false')
    expect(localStorage.getItem(TECH_KEY)).toBe('off')
    fireEvent.click(sw)
    expect(sw).toHaveAttribute('aria-checked', 'true')
    expect(localStorage.getItem(TECH_KEY)).toBe('on')
  })

  it('sits in the top bar next to the two text size buttons', () => {
    mount(<TopBar route={{ name: 'home' }} />, null)
    const bar = screen.getByRole('switch', { name: 'Technische namen' }).parentElement!
    expect(bar.querySelector('[role="group"][aria-label="Tekstgrootte"]')).not.toBeNull()
  })

  it('reads a stored "off" at the start and ignores a value it does not know', () => {
    localStorage.setItem(TECH_KEY, 'off')
    const first = mount(<TechNamesToggle />, null)
    expect(screen.getByRole('switch', { name: 'Technische namen' })).toHaveAttribute('aria-checked', 'false')
    first.unmount()
    localStorage.setItem(TECH_KEY, 'maybe')
    mount(<TechNamesToggle />, null)
    expect(screen.getByRole('switch', { name: 'Technische namen' })).toHaveAttribute('aria-checked', 'true')
  })

  it('shows and hides every line at once, wherever it is', () => {
    const { container } = mount(
      <>
        <TechNamesToggle />
        <ul>
          <ScenarioCard s={entry('switchover-date')} number={1} />
          <ScenarioCard s={entry(FLOW.id)} number={2} />
        </ul>
        <TechLine tech="one · two" />
      </>,
      null,
    )
    expect(lines(container)).toEqual(['dbt_env_vars · DBT_CARRIER_V2_FROM · stg_carrier_events', 'vdp_ci · state:modified+ · --empty', 'one · two'])
    fireEvent.click(screen.getByRole('switch', { name: 'Technische namen' }))
    expect(lines(container)).toEqual([])
    fireEvent.click(screen.getByRole('switch', { name: 'Technische namen' }))
    expect(lines(container)).toHaveLength(3)
  })

  it('draws nothing for an empty or missing tech', () => {
    const { container } = mount(<><TechLine tech={null} /><TechLine tech={[]} /><TechLine tech="  " /></>, null)
    expect(lines(container)).toEqual([])
    expect(techText(['a ', '', 'b'])).toBe('a · b')
  })
})

describe('where the line is shown', () => {
  it('is on a home card under the promise, and on a card without keywords there is none', () => {
    const { container } = mount(<ul><ScenarioCard s={entry('late-delivery')} number={1} /><ScenarioCard s={entry('skip-one-table')} number={2} /></ul>, null)
    const cards = container.querySelectorAll('li')
    const first = cards[0]!
    expect(first.querySelector('[data-tech]')?.textContent).toBe('dbt source freshness')
    expect(first.querySelector('[data-tech]')?.previousElementSibling?.textContent).toBe(entry('late-delivery').promise)   // right under the promise
    expect(cards[1]!.querySelector('[data-tech]')).toBeNull()
  })

  it('is in the header of a scenario, on all four parts, and hidden with the switch off', async () => {
    for (const part of ['problem', 'idea', 'code', 'demo'] as PartId[]) {
      const { container, unmount } = mount(<ScenarioPage id="switchover-date" part={part} />, null, full)
      await screen.findByRole('heading', { level: 1 })
      expect(container.querySelector('header [data-tech]')?.textContent, part).toBe('dbt_env_vars · DBT_CARRIER_V2_FROM · stg_carrier_events')
      unmount()
    }
    localStorage.setItem(TECH_KEY, 'off')
    const { container } = mount(<ScenarioPage id="switchover-date" part="problem" />, null, full)
    await screen.findByRole('heading', { level: 1 })
    expect(container.querySelector('[data-tech]')).toBeNull()
  })

  it('is on a step card that has tech, next to its feature badge, and not on one without', () => {
    const card = (i: number) => <StepCard scenarioId={FLOW.id} index={i} total={6} def={FLOW.demo.steps[i]!} state={null} blocked={null} values={{}} onValue={() => undefined} onRun={() => undefined} />
    const withTech = mount(card(4), null)
    expect(lines(withTech.container)).toEqual(['deploy_prd (manual)'])
    expect(withTech.container.querySelector('[data-status]')?.closest('div')?.querySelector('[data-tech]')).not.toBeNull()
    withTech.unmount()
    const without = mount(card(3), null)
    expect(lines(without.container)).toEqual([])
  })

  it('is on the closing page: a stage shows its own line, a scenario its keywords, both before "Without it"', () => {
    const { container } = mount(<Roadmap />, null, full)
    return screen.findByText('Nightly table build').then(() => {
      expect(lines(container)).toContain('dbt build --select tag:nightly')               // a flow stage with tech
      expect(lines(container)).toContain('dbt source freshness')                         // the stage "check" and the scenario "late-delivery"
      expect(lines(container)).toContain('dbt_env_vars · DBT_CARRIER_V2_FROM · stg_carrier_events')
      expect(lines(container)).not.toContain('vdp_ci · state:modified+ · --empty')       // a flow's own keywords are not repeated on its rows
      const row = screen.getByText('Step-by-step releases').closest('li')!
      const tech = row.querySelector('[data-tech]')!
      const why = row.querySelector('[class*="why"]')!
      expect(tech.compareDocumentPosition(why) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    })
  })

  it('is not on the closing page with the switch off', async () => {
    localStorage.setItem(TECH_KEY, 'off')
    const { container } = mount(<Roadmap />, null, full)
    await screen.findByText('Nightly table build')
    expect(lines(container)).toEqual([])
  })
})

describe('the scenario search', () => {
  const ids = (q: string) => searchScenarios(full, q).map((s) => s.id)

  it('finds a scenario by a keyword of the scenario or of one of its steps, in any case', () => {
    expect(ids('dbt_env_vars')).toEqual(['switchover-date'])
    expect(ids('CARRIER_V2')).toEqual(['switchover-date'])
    expect(ids('tag:nightly')).toEqual([FLOW.id])           // a step keyword finds its scenario
    expect(ids('DEPLOY_PRD')).toEqual([FLOW.id])
    expect(ids('merge request')).toEqual(['checked-before-merge'])
    expect(ids('freshness')).toEqual(['late-delivery', FLOW.id])   // a scenario keyword and a step keyword
  })

  it('still matches the words of the title and promise, and every word of the query has to match', () => {
    expect(ids('late')).toContain('late-delivery')
    expect(ids('freshness late')).toEqual(['late-delivery'])
    expect(ids('nothing-like-this')).toEqual([])
  })

  it('works with the switch off, because it reads the data and not the screen', async () => {
    localStorage.setItem(TECH_KEY, 'off')
    mount(<Switcher open onClose={() => undefined} currentId={null} />, null, full)
    const box = await screen.findByRole('combobox')
    fireEvent.change(box, { target: { value: 'TAG:NIGHTLY' } })
    expect(await screen.findByRole('option', { name: /From a new order to a trusted report/ })).toBeInTheDocument()
    expect(screen.getAllByRole('option')).toHaveLength(1)
  })
})

describe('the data', () => {
  it('has keywords for the scenario and a line for a step in the mock', () => {
    expect(SCENARIOS['switchover-date']!.tech).toEqual(['dbt_env_vars', 'DBT_CARRIER_V2_FROM', 'stg_carrier_events'])
    expect(FLOW.demo.steps.map((s) => s.tech ?? null)).toEqual([null, 'dbt source freshness', 'dbt build --select tag:nightly', null, 'deploy_prd (manual)', null])
  })
})
