import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, screen } from '@testing-library/react'
import { ScenarioCard, PREVIEW_DELAY_MS, PREVIEW_LEAVE_MS } from './ScenarioCard'
import { Home } from '../screens/Home'
import { mount } from '../test/harness'
import { previewIdea } from '../lib/visual'
import { CATALOG, withIdeas } from '../dev/mockContent'
import { withFlowAndRoadmap } from '../dev/mockFlow'
import { TECH_KEY } from '../lib/techNames'
import type { Catalog, CatalogScenario } from '../api/types'

const full: Catalog = withIdeas(withFlowAndRoadmap(CATALOG, () => true))
const all = full.groups.flatMap((g) => g.scenarios)
const entry = (id: string): CatalogScenario => all.find((s) => s.id === id)!
const flow = all.find((s) => s.kind === 'flow')!
const plain = entry('switchover-date')

const tile = (c: HTMLElement) => c.querySelector('li')!
const preview = (c: HTMLElement) => c.querySelector('[data-preview]')
const hover = (el: Element, type: string = 'mouse') => fireEvent.pointerEnter(el, { pointerType: type })
const unhover = (el: Element, type: string = 'mouse') => fireEvent.pointerLeave(el, { pointerType: type })
const wait = (ms: number) => act(() => void vi.advanceTimersByTime(ms))
const cards = (...s: CatalogScenario[]) => (
  <ul>
    {s.map((x, i) => (
      <ScenarioCard key={x.id} s={x} number={i + 1} />
    ))}
  </ul>
)

beforeEach(() => {
  localStorage.clear()
  vi.useFakeTimers()
})
afterEach(() => vi.useRealTimers())

describe('the tile is a link', () => {
  it('is one link to part 1, named by the title, with no button in the tile', () => {
    const { container } = mount(cards(plain), null)
    const link = screen.getByRole('link', { name: plain.title })
    expect(link).toHaveAttribute('href', `#/s/${plain.id}/problem`)
    expect(link.closest('h3')).not.toBeNull()
    expect(container.querySelector('button')).toBeNull()
    expect(screen.queryByText('Start')).toBeNull()
  })

  it('keeps "Direct naar de demo" as its own link, not inside the tile link', () => {
    const { container } = mount(cards(plain), null)
    const demo = screen.getByRole('link', { name: `Direct naar de demo: ${plain.title}` })
    expect(demo).toHaveAttribute('href', `#/s/${plain.id}/demo`)
    const main = screen.getByRole('link', { name: plain.title })
    expect(main.contains(demo)).toBe(false)
    expect(demo.contains(main)).toBe(false)
    expect(container.querySelectorAll('a a, a button, button a, button button')).toHaveLength(0)
  })

  it('describes the link by the promise and shows no glossary button inside the tile', () => {
    const withTerm = { ...plain, title: 'About [[feeds]] today', promise: 'Every [[feed]] counts.' }
    const { container } = mount(cards(withTerm), null)
    const link = screen.getByRole('link', { name: 'About feeds today' })
    expect(link.getAttribute('aria-describedby')).toBe(container.querySelector('p[id]')!.id)
    expect(container.querySelector('p[id]')!.textContent).toBe('Every feed counts.')
    expect(container.querySelector('button')).toBeNull()
  })

  it('keeps the numbers, the flow label and the badges', () => {
    mount(cards(flow, { ...plain, status: 'today', feature: 'x' }), null)
    expect(screen.getByText('Flow · 6 stappen')).toBeInTheDocument()
    expect(screen.getByText('Vandaag al in gebruik')).toBeInTheDocument()
    expect(screen.getByText('01')).toBeInTheDocument()
  })
})

describe('the preview', () => {
  it('shows the idea after a short delay of the pointer on the tile, and hides it again after it leaves', () => {
    const { container } = mount(cards(plain), null)
    expect(preview(container)).toBeNull()
    hover(tile(container))
    wait(PREVIEW_DELAY_MS - 50)
    expect(preview(container)).toBeNull()
    wait(60)
    const p = preview(container)!
    expect(p).not.toBeNull()
    expect(p.textContent).toContain('Het idee')
    expect(p.textContent).toContain(plain.idea!.headline)
    expect(p).toHaveAttribute('aria-hidden', 'true')
    expect(tile(container)).toHaveAttribute('data-previewing', '')
    unhover(tile(container))
    wait(PREVIEW_LEAVE_MS - 10)
    expect(preview(container)).not.toBeNull()                 // a short grace: a slip does not flicker
    wait(20)
    expect(preview(container)).toBeNull()
    expect(tile(container).hasAttribute('data-previewing')).toBe(false)
  })

  it('shows nothing when the pointer only passes over the tile', () => {
    const { container } = mount(cards(plain), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS - 100)
    unhover(tile(container))
    wait(2000)
    expect(preview(container)).toBeNull()
  })

  it('stays while the pointer goes back onto the tile within the grace', () => {
    const { container } = mount(cards(plain), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS + 10)
    unhover(tile(container))
    wait(PREVIEW_LEAVE_MS - 40)
    hover(tile(container))
    wait(1000)
    expect(preview(container)).not.toBeNull()
  })

  it('comes quicker when the presenter goes from one tile to the next', () => {
    const { container } = mount(cards(plain, entry('late-delivery')), null)
    const [a, b] = Array.from(container.querySelectorAll('li'))
    hover(a!)
    wait(PREVIEW_DELAY_MS + 10)
    unhover(a!)
    wait(PREVIEW_LEAVE_MS + 10)
    expect(preview(container)).toBeNull()
    hover(b!)
    wait(200)
    expect(b!.querySelector('[data-preview]')).not.toBeNull()
  })

  it('never shows for a touch pointer: a tap just opens the scenario', () => {
    const { container } = mount(cards(plain), null)
    hover(tile(container), 'touch')
    wait(2000)
    expect(preview(container)).toBeNull()
  })

  it('shows on keyboard focus at once, stays while focus is in the tile, and hides when focus leaves', () => {
    const { container } = mount(cards(plain, entry('late-delivery')), null)
    const [a, b] = Array.from(container.querySelectorAll('li'))
    const link = screen.getAllByRole('link')[0]!
    act(() => link.focus())
    expect(a!.querySelector('[data-preview]')).not.toBeNull()
    const demo = screen.getAllByRole('link')[1]!
    act(() => demo.focus())                                   // the second link of the same tile
    expect(a!.querySelector('[data-preview]')).not.toBeNull()
    act(() => screen.getAllByRole('link').find((l) => b!.contains(l))!.focus())
    expect(a!.querySelector('[data-preview]')).toBeNull()
    expect(b!.querySelector('[data-preview]')).not.toBeNull()
  })

  it('closes with Escape and has no tab stop of its own', () => {
    const { container } = mount(cards(plain), null)
    act(() => screen.getAllByRole('link')[0]!.focus())
    const p = preview(container)!
    expect(p).toHaveAttribute('tabindex', '-1')
    expect(p.getAttribute('href')).toBe(`#/s/${plain.id}/problem`)       // a click on it opens the scenario too
    fireEvent.keyDown(screen.getAllByRole('link')[0]!, { key: 'Escape' })
    expect(preview(container)).toBeNull()
  })

  it('has no nested links or buttons, and a [[term]] in it is a plain word', () => {
    const idea = { headline: 'The [[feed]] arrives', visual: { type: 'flow' as const, nodes: [{ id: 'a', label: 'Every [[feed]]', state: 'ok' as const }, { id: 'b', label: 'Check', state: 'fix' as const }], links: [['a', 'b'] as [string, string]] } }
    const { container } = mount(cards({ ...plain, idea }), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS + 10)
    const p = preview(container)!
    expect(p.querySelectorAll('a, button')).toHaveLength(0)
    expect(container.querySelectorAll('a a, a button')).toHaveLength(0)
    expect(p.textContent).toContain('The feed arrives')
    expect(p.textContent).toContain('Every feed')
  })

  it('draws the small flow of the idea, and a comparison or a timeline when that is the idea', () => {
    const compare = { headline: 'Two ways', visual: { type: 'compare' as const, left: { title: 'Alone', items: ['x'], state: 'bad' as const }, right: { title: 'Together', items: ['y'], state: 'ok' as const } } }
    const timeline = { headline: 'Over a week', visual: { type: 'timeline' as const, days: Array.from({ length: 5 }, (_, i) => ({ label: `Day ${i}`, state: 'ok' as const })), marker: { after: 2, label: 'Switch' } } }
    for (const [idea, text] of [[plain.idea!, 'Het idee'], [compare, 'Together'], [timeline, 'Switch']] as const) {
      const { container, unmount } = mount(cards({ ...plain, idea }), null)
      hover(tile(container))
      wait(PREVIEW_DELAY_MS + 10)
      expect(preview(container)!.querySelector('figure[data-compact]')).not.toBeNull()
      expect(preview(container)!.textContent).toContain(text)
      unmount()
    }
  })

  it('keeps the state word of a node for screen readers but does not show it in the compact chip', () => {
    const idea = { headline: 'h', visual: { type: 'flow' as const, nodes: [{ id: 'a', label: 'Voorstel', state: 'idle' as const }, { id: 'b', label: 'Check', state: 'ok' as const }, { id: 'c', label: 'Live', state: 'ok' as const }], links: [['a', 'b'] as [string, string], ['b', 'c'] as [string, string]] } }
    const { container } = mount(cards({ ...plain, idea }), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS + 10)
    const word = preview(container)!.querySelector('.state > span:last-child')!
    expect(word.textContent).toBe('Wacht')          // still in the markup; the compact stylesheet clips it
  })

  it('lists the stages of a flow under the two status labels, each label once', () => {
    const { container } = mount(cards(flow), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS + 10)
    const stages = preview(container)!.querySelector('[data-stages]')!
    expect(stages.querySelectorAll('[data-status]')).toHaveLength(2)
    expect(stages.textContent).toContain('Vandaag al in gebruik')
    expect(stages.textContent).toContain('Daily data delivery')
    expect(stages.textContent).toContain('Volgende stap')
    expect(stages.textContent).toContain('Private test copy')
  })

  it('lists no stages for an ordinary scenario', () => {
    const { container } = mount(cards({ ...plain, status: 'today', feature: 'x' }), null)
    hover(tile(container))
    wait(PREVIEW_DELAY_MS + 10)
    expect(preview(container)!.querySelector('[data-stages]')).toBeNull()
  })

  it('is left out for a tile without an idea (an older backend) or one with a visual it cannot draw, which stays a link', () => {
    const none = { ...plain, idea: undefined }
    const odd = { ...plain, id: 'odd', idea: { headline: 'h', visual: { type: 'hologram' } as never } }
    const empty = { ...plain, id: 'empty', idea: { headline: 'h', visual: { type: 'flow', nodes: [], links: [] } as never } }
    const { container } = mount(cards(none, odd, empty), null)
    for (const li of Array.from(container.querySelectorAll('li'))) {
      hover(li)
      wait(PREVIEW_DELAY_MS + 10)
      expect(li.querySelector('[data-preview]')).toBeNull()
    }
    expect(screen.getAllByRole('link').filter((l) => l.getAttribute('href')!.endsWith('/problem'))).toHaveLength(3)
    expect(previewIdea(undefined)).toBeNull()
    expect(previewIdea({ headline: 'x', visual: plain.idea!.visual })).not.toBeNull()
  })

  it('works with the technical line off as well as on', () => {
    for (const v of ['on', 'off']) {
      localStorage.setItem(TECH_KEY, v)
      const { container, unmount } = mount(cards(plain), null)
      expect(!!container.querySelector('[data-tech]')).toBe(v === 'on')
      act(() => screen.getAllByRole('link')[0]!.focus())
      expect(preview(container)).not.toBeNull()
      unmount()
    }
  })
})

describe('the home page', () => {
  it('has a link tile per scenario, one "Begin hier", the flow label and no Start button', async () => {
    vi.useRealTimers()
    const { container } = mount(<Home />, null, full)
    await screen.findByText('Flow · 6 stappen')
    const items = container.querySelectorAll('ul li')
    expect(items.length).toBe(all.length)
    expect(screen.getAllByText('Begin hier')).toHaveLength(1)
    expect(screen.queryAllByRole('button', { name: /^Start/ })).toHaveLength(0)
    expect(container.querySelectorAll('li a[href$="/problem"]')).toHaveLength(all.length)
    expect(container.querySelectorAll('li a[href$="/demo"]')).toHaveLength(all.length)
  })
})
