import { describe, expect, it } from 'vitest'
import { screen, within } from '@testing-library/react'
import { Roadmap } from './Roadmap'
import { Home } from './Home'
import { mount } from '../test/harness'
import { buildRoadmap } from '../lib/roadmap'
import { parseHash, toHash } from '../lib/router'
import { CATALOG } from '../dev/mockContent'
import { ROADMAP, withFlowAndRoadmap } from '../dev/mockFlow'
import type { Catalog } from '../api/types'

const full = withFlowAndRoadmap(CATALOG, () => true)
const bare: Catalog = { ...structuredClone(CATALOG), roadmap: { title: 'Where we stand', intro: 'One sentence.', extras: [] } }

describe('buildRoadmap', () => {
  it('has two sections, in place today first, each with its groups in home-page order and the extras by area last', () => {
    const [today, next] = buildRoadmap(full).sections
    expect([today.status, next.status]).toEqual(['today', 'next'])
    expect(today.groups.map((g) => g.id)).toEqual(['sources', 'flows'])         // a group with no row in the section is left out
    expect(today.groups[0]!.blocks[0]!.rows.map((r) => r.feature)).toEqual(['A late feed is reported', 'Feeds that change format'])
    expect(today.groups[1]!.blocks[0]!.title).toBe('From a new order to a trusted report')
    expect(today.groups[1]!.blocks[0]!.rows.map((r) => r.feature)).toEqual(['Daily data delivery', 'Automatic data checks', 'Nightly table build'])
    expect(today.areas.map((a) => [a.area, a.rows.length])).toEqual([['Access', 1]])
    expect(next.groups.map((g) => g.id)).toEqual(['releases', 'flows'])
    expect(next.groups[0]!.blocks[0]!.rows.map((r) => r.feature)).toEqual(['Every environment has its own keys'])
    expect(next.groups[1]!.blocks[0]!.rows.map((r) => r.feature)).toEqual(['Private test copy', 'Step-by-step releases'])   // the last stage has none
    expect(next.areas.map((a) => [a.area, a.rows.length])).toEqual([['Access', 1]])
  })

  it('counts what is in place today and what is next, extras included', () => {
    const view = buildRoadmap(full)
    expect(view).toMatchObject({ today: 6, next: 4, total: 10 })
    expect(view.sections.map((s) => s.count)).toEqual([6, 4])
  })

  it('carries a why on the rows that have one and on no other', () => {
    const [today, next] = buildRoadmap(full).sections
    expect(next.groups[0]!.blocks[0]!.rows[0]!.why).toBe('One leaked key opens every environment.')
    expect(next.groups[1]!.blocks[0]!.rows.map((r) => r.why)).toEqual(['A change goes live without anybody having seen it work.', 'A release is a manual job, so it waits for one person.'])
    expect(next.areas[0]!.rows[0]!.why).toBe('Every team can read the data of the other teams.')
    expect(today.groups[0]!.blocks[0]!.rows.every((r) => r.why === undefined)).toBe(true)
  })

  it('joins ordinary scenarios that sit next to each other once a flow between them has gone', () => {
    const odd = withFlowAndRoadmap(CATALOG, () => true)
    const sources = odd.groups.find((g) => g.id === 'sources')!
    const flow = structuredClone(odd.groups.at(-1)!.scenarios[0]!)
    sources.scenarios.splice(1, 0, { ...flow, id: 'flow-in-the-middle', steps: flow.steps!.map((s) => ({ ...s, status: 'next' as const })) })
    const [today, next] = buildRoadmap(odd).sections
    expect(today.groups[0]!.blocks).toHaveLength(1)                              // the flow has no stage in place today
    expect(next.groups[0]!.blocks.map((b) => b.title)).toEqual(['From a new order to a trusted report'])
  })

  it('has nothing to list when no status was given', () => {
    expect(buildRoadmap(bare)).toMatchObject({ today: 0, next: 0, total: 0 })
    expect(buildRoadmap(bare).sections.map((s) => [s.groups, s.areas])).toEqual([[[], []], [[], []]])
    expect(buildRoadmap(null)).toMatchObject({ total: 0 })
  })

  it('ignores an unknown status and a flow stage without a feature', () => {
    const odd = withFlowAndRoadmap(CATALOG, () => true)
    const flow = odd.groups.at(-1)!.scenarios[0]!
    flow.steps![0]!.status = 'someday' as never
    flow.steps![1]!.feature = null
    const [today, next] = buildRoadmap(odd).sections
    expect(today.groups.at(-1)!.blocks[0]!.rows).toHaveLength(1)
    expect(next.groups.at(-1)!.blocks[0]!.rows).toHaveLength(2)
  })

  it('works with a catalog from an older server: no why anywhere', () => {
    const old = withFlowAndRoadmap(CATALOG, () => true)
    for (const s of old.groups.flatMap((g) => g.scenarios)) {
      delete s.why
      s.steps?.forEach((st) => delete st.why)
    }
    old.roadmap!.extras.forEach((x) => delete x.why)
    const rows = buildRoadmap(old).sections.flatMap((s) => [...s.groups.flatMap((g) => g.blocks.flatMap((b) => b.rows)), ...s.areas.flatMap((a) => a.rows)])
    expect(rows).toHaveLength(10)
    expect(rows.every((r) => !r.why)).toBe(true)
  })
})

describe('the roadmap route', () => {
  it('has its own address', () => {
    expect(parseHash('#/roadmap')).toEqual({ name: 'roadmap' })
    expect(toHash({ name: 'roadmap' })).toBe('#/roadmap')
    expect(parseHash('#/roadmap/extra')).toEqual({ name: 'unknown' })
  })
})

describe('the closing overview page', () => {
  it('shows the title, the intro and the closing sentence', async () => {
    mount(<Roadmap />, null, full)
    expect(await screen.findByRole('heading', { level: 1, name: ROADMAP.title })).toBeInTheDocument()
    expect(screen.getByText(ROADMAP.intro)).toBeInTheDocument()
    expect(screen.getByText(ROADMAP.closing!)).toBeInTheDocument()
  })

  it('has two sections, each with its count in the heading, in place today first', async () => {
    const { container } = mount(<Roadmap />, null, full)
    expect(await screen.findByRole('heading', { level: 2, name: 'Vandaag al in gebruik (6)' })).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 2, name: 'Volgende stappen (4)' })).toBeInTheDocument()
    expect([...container.querySelectorAll('h2')].map((h) => h.textContent)).toEqual(['Vandaag al in gebruik (6)', 'Volgende stappen (4)'])
  })

  it('groups the rows of a section under the group and flow titles, and the extras under their area at the end', async () => {
    const { container } = mount(<Roadmap />, null, full)
    await screen.findByRole('heading', { level: 1 })
    const today = container.querySelector('section[data-status="today"]') as HTMLElement
    const next = container.querySelector('section[data-status="next"]') as HTMLElement
    expect([...today.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['When the data changes', 'The whole route', 'Access'])
    expect([...next.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['When we release', 'The whole route', 'Access'])
    expect(within(today).getByRole('link', { name: 'From a new order to a trusted report' })).toHaveAttribute('href', '#/s/flow-test/problem')
    expect(within(next).getByRole('link', { name: 'From a new order to a trusted report' })).toHaveAttribute('href', '#/s/flow-test/problem')
    expect(within(today).getAllByRole('listitem').map((li) => li.querySelector('a, [class*="name"]')?.textContent)).toEqual([
      'A late feed is reported', 'Feeds that change format', 'Daily data delivery', 'Automatic data checks', 'Nightly table build', 'A named person approves every release'])
  })

  it('puts each row in the section of its status and links it to its scenario', async () => {
    mount(<Roadmap />, null, full)
    await screen.findByRole('heading', { level: 1 })
    const today = screen.getByRole('heading', { level: 2, name: /Vandaag al in gebruik/ }).closest('section')!
    const next = screen.getByRole('heading', { level: 2, name: /Volgende stappen/ }).closest('section')!
    const stage = within(next).getByRole('link', { name: 'Private test copy' })
    expect(stage).toHaveAttribute('href', '#/s/flow-test/demo')
    expect(within(today).queryByRole('link', { name: 'Private test copy' })).toBeNull()
    expect(within(today).getByRole('link', { name: 'Daily data delivery' })).toHaveAttribute('href', '#/s/flow-test/demo')
    expect(within(next).queryByRole('link', { name: 'Daily data delivery' })).toBeNull()
    expect(screen.queryByText('Everything switched on')).toBeNull()                 // the last stage has no feature
    expect(within(today).getByRole('link', { name: 'A late feed is reported' })).toHaveAttribute('href', '#/s/late-delivery/problem')
    expect(screen.queryByText('Volgende stap')).toBeNull()                              // no badge per row: the section says it
  })

  it('shows an extra under its area, without a link, with its note', async () => {
    mount(<Roadmap />, null, full)
    const next = (await screen.findByRole('heading', { level: 2, name: /Volgende stappen/ })).closest('section')!
    const extra = within(next).getByText('Each team sees only its own data').closest('li')!
    expect(within(extra).getByText(/one more round of agreements/)).toBeInTheDocument()
    expect(within(extra).queryByRole('link')).toBeNull()                            // an extra has no scenario to link to
    expect(within(next).getByRole('heading', { level: 3, name: 'Access' })).toBeInTheDocument()
  })

  it('shows a why as a second line "Zonder: ..." under the feature name, only on the rows that have one', async () => {
    mount(<Roadmap />, null, full)
    await screen.findByRole('heading', { level: 1 })
    const stage = screen.getByRole('link', { name: 'Private test copy' }).closest('li')!
    expect(stage.textContent).toBe('Private test copyZonder: A change goes live without anybody having seen it work.')
    const extra = screen.getByText('Each team sees only its own data').closest('li')!
    expect(extra.textContent).toContain('Zonder: Every team can read the data of the other teams.')
    expect(screen.getByRole('link', { name: 'Every environment has its own keys' }).closest('li')!.textContent).toContain('Zonder: One leaked key opens every environment.')
    expect(screen.getByRole('link', { name: 'Nightly table build' }).closest('li')!.textContent).not.toContain('Zonder')
    expect(screen.getAllByText('Zonder:', { exact: false })).toHaveLength(5)    // three stages, one scenario, one extra
  })

  it('says so in a section that has no row', async () => {
    const onlyNext = structuredClone(full)
    for (const s of onlyNext.groups.flatMap((g) => g.scenarios)) {
      if (s.status === 'today') s.status = null
      s.steps?.forEach((st) => { if (st.status === 'today') st.status = null })
    }
    onlyNext.roadmap!.extras = onlyNext.roadmap!.extras.filter((x) => x.status === 'next')
    mount(<Roadmap />, null, onlyNext)
    expect(await screen.findByRole('heading', { level: 2, name: 'Vandaag al in gebruik (0)' })).toBeInTheDocument()
    expect(screen.getByText('Er is nog niets gemarkeerd als in gebruik.')).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 2, name: 'Volgende stappen (4)' })).toBeInTheDocument()
  })

  it('keeps the closing sentence after the sections', async () => {
    const { container } = mount(<Roadmap />, null, full)
    const closing = await screen.findByText(ROADMAP.closing!)
    const sections = container.querySelectorAll('section')
    expect(sections[sections.length - 1]!.compareDocumentPosition(closing) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('says in one plain sentence that nothing has a status yet', async () => {
    mount(<Roadmap />, null, bare)
    expect(await screen.findByText('Er is nog niets gemarkeerd als al in gebruik of als volgende stap.')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 2 })).toBeNull()
  })

  it('says so when there is no roadmap file', async () => {
    mount(<Roadmap />, null, { ...structuredClone(CATALOG), roadmap: null })
    expect(await screen.findByText('Er is nog geen slotoverzicht')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: "Terug naar alle scenario's" })).toHaveAttribute('href', '#/')
  })
})

describe('the closing card on the home page', () => {
  it('is one distinct card at the bottom, linking to the overview', async () => {
    const { container } = mount(<Home />, null, full)
    const link = await screen.findByRole('link', { name: `Open: ${ROADMAP.title}` })
    expect(link).toHaveAttribute('href', '#/roadmap')
    const cards = container.querySelectorAll('[data-closing-card]')
    expect(cards).toHaveLength(1)
    expect(container.querySelector('[data-closing-card]')!.previousElementSibling!.tagName).toBe('SECTION')     // after the last group
    expect(container.querySelector('[data-closing-card]')!.nextElementSibling).toBeNull()
  })
  it('shows the flow card marked "Flow · 6 stappen"', async () => {
    mount(<Home />, null, full)
    expect(await screen.findByText('Flow · 6 stappen')).toBeInTheDocument()
  })
  it('is not there without a roadmap', async () => {
    const { container } = mount(<Home />, null, { ...structuredClone(CATALOG), roadmap: null })
    await screen.findByText(/korte verhalen/)
    await screen.findAllByRole('heading', { level: 2 })
    expect(container.querySelector('[data-closing-card]')).toBeNull()
  })
})
