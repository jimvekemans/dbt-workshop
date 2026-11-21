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
  it('has one position ("Scenario n van m") only in the header, never in the eyebrow', async () => {
    mount(
      <>
        <TopBar route={{ name: 'scenario', id: sc.id, part: 'problem' }} />
        <ScenarioPage id={sc.id} part="problem" />
      </>,
    )
    expect(await screen.findByText(/^Scenario 4 van \d+$/)).toBeInTheDocument()
    const eyebrow = (await screen.findAllByText(/ongeveer 4 min, inclusief demo/))[0]!
    expect(eyebrow.textContent).toContain('When the data changes')
    expect(eyebrow.textContent).not.toMatch(/van \d+/)
  })

  it('has a one-line footer with Back and a Next that says why, per part', async () => {
    mount(<ScenarioPage id={sc.id} part="problem" />)
    const next = await screen.findByRole('link', { name: /Volgende: Deel 2 · Het idee/ })
    expect(next).toHaveTextContent('hoe je het voorkomt')
    expect(screen.getByRole('link', { name: /Terug naar alle scenario's/ })).toBeInTheDocument()
  })
  it('hints "hoe het eruitziet in het project" and "kijk hoe het gebeurt"', async () => {
    mount(<ScenarioPage id={sc.id} part="idea" />)
    expect(await screen.findByRole('link', { name: /Volgende: Deel 3 · De code/ })).toHaveTextContent('hoe het eruitziet in het project')
  })
  it('asks the CSS for the sticky bar by marking the page', async () => {
    const { unmount } = mount(<ScenarioPage id={sc.id} part="problem" />)
    await screen.findByRole('link', { name: /Volgende: Deel 2 · Het idee/ })
    expect(document.documentElement.hasAttribute('data-sticky-footer')).toBe(true)
    unmount()
    expect(document.documentElement.hasAttribute('data-sticky-footer')).toBe(false)
  })

  it('puts related scenarios under the content as "Verwant, niet verplicht:"', async () => {
    mount(<ScenarioPage id={sc.id} part="idea" />)
    expect(await screen.findByText('Verwant, niet verplicht:')).toBeInTheDocument()
    const link = screen.getAllByRole('link').find((a) => a.getAttribute('href')?.includes('?from=switchover-date'))
    expect(link).toBeDefined()
  })

  it('offers "Terug naar: <scenario>" when the visitor came through a related link', async () => {
    mount(<ScenarioPage id={sc.id} part="problem" from="skip-one-table" />)
    expect(await screen.findByRole('link', { name: /Terug naar: One broken feed must not stop every report/ })).toBeInTheDocument()
  })

  it('on the last part the primary button goes to the next scenario, and is hidden while the demo is made ready', async () => {
    const { push } = mount(<ScenarioPage id={sc.id} part="demo" />, null)
    await push(baseServer)
    expect(await screen.findByRole('link', { name: /Volgende: .+/ })).toBeInTheDocument()
    await push({ ...baseServer, job: { kind: 'prepare', scenario: sc.id, step: null, label: 'Getting ready', started: 1, cancellable: true }, demo: { scenario: sc.id, status: 'preparing', message: '', steps: [] } })
    expect(screen.queryByRole('link', { name: /Volgende: .+/ })).toBeNull()
    expect(screen.queryByText('Verwant, niet verplicht:')).toBeNull()
  })

  it('a scenario that does not exist says so, with the right buttons and no "Opnieuw proberen"', async () => {
    mount(<ScenarioPage id="no-such-scenario" part="demo" />)
    expect(await screen.findByRole('heading', { level: 1, name: 'Dit scenario bestaat niet' })).toBeInTheDocument()
    expect(screen.getByText('De link is misschien oud. Kies er een uit de lijst.')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: "Terug naar alle scenario's" })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Ga naar een scenario' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Opnieuw proberen' })).toBeNull()
  })
})

describe('TopBar', () => {
  const route = { name: 'scenario', id: sc.id, part: 'problem' } as const
  it('has a name for every icon-only control and keeps the running job out of the header', async () => {
    const { push } = mount(<TopBar route={route} />, null)
    await push({ ...baseServer, job: { kind: 'step', scenario: sc.id, step: 'with', label: 'x', started: 1, cancellable: true } })
    await screen.findByText(/^Scenario 4 van \d+$/)
    expect(screen.getByRole('button', { name: "Scenario's: ga naar een scenario" })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^Vorig scenario: / })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^Volgend scenario: / })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Schakel over naar de donkere weergave' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Live. Wijzig hoe de demo\'s draaien' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Stop/ })).toBeNull()
  })
  it('shows no counter or arrows for a scenario that is not in the list', async () => {
    mount(<TopBar route={{ name: 'scenario', id: 'nope', part: 'problem' }} />)
    await screen.findByRole('button', { name: /Scenario's/ })
    expect(screen.queryByText(/Scenario \d+ van/)).toBeNull()
    expect(screen.queryByRole('button', { name: /^Volgend scenario/ })).toBeNull()
  })
  it('says Offline instead of the mode when the studio cannot be reached', async () => {
    const { connection, push } = mount(<TopBar route={{ name: 'home' }} />, null)
    await push(baseServer)
    await connection(false)
    expect(screen.getByText('Offline')).toBeInTheDocument()
  })
})

describe('Switcher', () => {
  it('counts the scenarios in its title and marks the current one "(je bent hier)"', async () => {
    mount(<Switcher open onClose={() => {}} currentId={sc.id} />)
    expect(await screen.findByRole('dialog', { name: 'Ga naar een scenario (16)' })).toBeInTheDocument()
    expect(screen.getByText(/\(je bent hier\)/)).toBeInTheDocument()
    expect(screen.getAllByRole('option')).toHaveLength(16)
  })
  it('keeps the empty message outside the listbox, offers "Zoekopdracht wissen" and hides the key hint', async () => {
    mount(<Switcher open onClose={() => {}} currentId={null} />)
    const input = await screen.findByRole('combobox')
    await screen.findAllByRole('option')
    fireEvent.change(input, { target: { value: 'zzzzqq' } })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByText(/Niets komt overeen met “zzzzqq”. Wis de zoekopdracht om alle 16 te zien./)).toBeInTheDocument()
    expect(screen.queryByText(/om te navigeren/)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Zoekopdracht wissen' }))
    expect(screen.getAllByRole('option')).toHaveLength(16)
  })
})

describe('ConnectionBanner', () => {
  it('says what is wrong, offers "Nu proberen", and shows a green "Weer online" when the link returns', async () => {
    const { connection, backend } = mount(<ConnectionBanner />)
    await connection(false)
    expect(screen.getByRole('alert')).toHaveTextContent('Het oefensysteem is niet bereikbaar')
    expect(screen.getByText(/Nieuwe poging binnen \d s/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Nu proberen' }))
    expect(backend).toBeDefined()
    await connection(true)
    expect(await screen.findByText('Weer online')).toBeInTheDocument()
    expect(screen.queryByText('Het oefensysteem is niet bereikbaar')).toBeNull()
  })
})
