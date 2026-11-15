import { describe, expect, it } from 'vitest'
import { fireEvent, screen } from '@testing-library/react'
import { ScenarioPage } from './ScenarioPage'
import { TopBar } from '../components/TopBar'
import { Switcher } from '../components/Switcher'
import { ConnectionBanner } from '../components/ConnectionBanner'
import { baseServer, mount } from '../test/harness'
import { SCENARIOS } from '../dev/mockContent'

const sc = SCENARIOS['switchover-date']!

describe('ScenarioPage', () => {
  it('has one position ("Scenario n of m") only in the header, never in the eyebrow', async () => {
    mount(
      <>
        <TopBar route={{ name: 'scenario', id: sc.id, part: 'problem' }} />
        <ScenarioPage id={sc.id} part="problem" />
      </>,
    )
    expect(await screen.findByText(/^Scenario 4 of \d+$/)).toBeInTheDocument()
    const eyebrow = (await screen.findAllByText(/about 4 min, including the demo/))[0]!
    expect(eyebrow.textContent).toContain('When the data changes')
    expect(eyebrow.textContent).not.toMatch(/of \d+/)
  })

  it('has a one-line footer with Back and a Next that says why, per part', async () => {
    mount(<ScenarioPage id={sc.id} part="problem" />)
    const next = await screen.findByRole('link', { name: /Next: Part 2 · The idea/ })
    expect(next).toHaveTextContent('how it is prevented')
    expect(screen.getByRole('link', { name: /Back to all scenarios/ })).toBeInTheDocument()
  })
  it('hints "what it looks like in the project" and "watch it happen"', async () => {
    mount(<ScenarioPage id={sc.id} part="idea" />)
    expect(await screen.findByRole('link', { name: /Next: Part 3 · The code/ })).toHaveTextContent('what it looks like in the project')
  })
  it('asks the CSS for the sticky bar by marking the page', async () => {
    const { unmount } = mount(<ScenarioPage id={sc.id} part="problem" />)
    await screen.findByRole('link', { name: /Next: Part 2 · The idea/ })
    expect(document.documentElement.hasAttribute('data-sticky-footer')).toBe(true)
    unmount()
    expect(document.documentElement.hasAttribute('data-sticky-footer')).toBe(false)
  })

  it('puts related scenarios under the content as "Related, optional:"', async () => {
    mount(<ScenarioPage id={sc.id} part="idea" />)
    expect(await screen.findByText('Related, optional:')).toBeInTheDocument()
    const link = screen.getAllByRole('link').find((a) => a.getAttribute('href')?.includes('?from=switchover-date'))
    expect(link).toBeDefined()
  })

  it('offers "Back to: <scenario>" when the visitor came through a related link', async () => {
    mount(<ScenarioPage id={sc.id} part="problem" from="skip-one-table" />)
    expect(await screen.findByRole('link', { name: /Back to: One broken feed must not stop every report/ })).toBeInTheDocument()
  })

  it('on the last part the primary button goes to the next scenario, and is hidden while the demo is made ready', async () => {
    const { push } = mount(<ScenarioPage id={sc.id} part="demo" />, null)
    await push(baseServer)
    expect(await screen.findByRole('link', { name: /Next: .+/ })).toBeInTheDocument()
    await push({ ...baseServer, job: { kind: 'prepare', scenario: sc.id, step: null, label: 'Getting ready', started: 1, cancellable: true }, demo: { scenario: sc.id, status: 'preparing', message: '', steps: [] } })
    expect(screen.queryByRole('link', { name: /Next: .+/ })).toBeNull()
    expect(screen.queryByText('Related, optional:')).toBeNull()
  })

  it('a scenario that does not exist says so, with the right buttons and no "Try again"', async () => {
    mount(<ScenarioPage id="no-such-scenario" part="demo" />)
    expect(await screen.findByRole('heading', { level: 1, name: 'This scenario does not exist' })).toBeInTheDocument()
    expect(screen.getByText('The link may be old. Pick one from the list.')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Back to all scenarios' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Go to a scenario' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Try again' })).toBeNull()
  })
})

describe('TopBar', () => {
  const route = { name: 'scenario', id: sc.id, part: 'problem' } as const
  it('has a name for every icon-only control and keeps the running job out of the header', async () => {
    const { push } = mount(<TopBar route={route} />, null)
    await push({ ...baseServer, job: { kind: 'step', scenario: sc.id, step: 'with', label: 'x', started: 1, cancellable: true } })
    await screen.findByText(/^Scenario 4 of \d+$/)
    expect(screen.getByRole('button', { name: 'Scenarios: go to a scenario' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^Previous scenario: / })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^Next scenario: / })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Switch to dark mode' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Live. Change how the demos run' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull()
  })
  it('shows no counter or arrows for a scenario that is not in the list', async () => {
    mount(<TopBar route={{ name: 'scenario', id: 'nope', part: 'problem' }} />)
    await screen.findByRole('button', { name: /Scenarios/ })
    expect(screen.queryByText(/Scenario \d+ of/)).toBeNull()
    expect(screen.queryByRole('button', { name: /^Next scenario/ })).toBeNull()
  })
  it('says Offline instead of the mode when the studio cannot be reached', async () => {
    const { connection, push } = mount(<TopBar route={{ name: 'home' }} />, null)
    await push(baseServer)
    await connection(false)
    expect(screen.getByText('Offline')).toBeInTheDocument()
  })
})

describe('Switcher', () => {
  it('counts the scenarios in its title and marks the current one "(you are here)"', async () => {
    mount(<Switcher open onClose={() => {}} currentId={sc.id} />)
    expect(await screen.findByRole('dialog', { name: 'Go to a scenario (16)' })).toBeInTheDocument()
    expect(screen.getByText(/\(you are here\)/)).toBeInTheDocument()
    expect(screen.getAllByRole('option')).toHaveLength(16)
  })
  it('keeps the empty message outside the listbox, offers "Clear search" and hides the key hint', async () => {
    mount(<Switcher open onClose={() => {}} currentId={null} />)
    const input = await screen.findByRole('combobox')
    await screen.findAllByRole('option')
    fireEvent.change(input, { target: { value: 'zzzzqq' } })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByText(/Nothing matches “zzzzqq”. Clear the search to see all 16./)).toBeInTheDocument()
    expect(screen.queryByText(/to move/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }))
    expect(screen.getAllByRole('option')).toHaveLength(16)
  })
})

describe('ConnectionBanner', () => {
  it('says what is wrong, offers "Try now", and shows a green "Back online" when the link returns', async () => {
    const { connection, backend } = mount(<ConnectionBanner />)
    await connection(false)
    expect(screen.getByRole('alert')).toHaveTextContent('The demo system cannot be reached')
    expect(screen.getByText(/Trying again in \d s/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Try now' }))
    expect(backend).toBeDefined()
    await connection(true)
    expect(await screen.findByText('Back online')).toBeInTheDocument()
    expect(screen.queryByText('The demo system cannot be reached')).toBeNull()
  })
})
