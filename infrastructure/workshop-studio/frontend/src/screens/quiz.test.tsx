import { beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { Question } from './Question'
import { QuizPage } from './QuizPage'
import { ScenarioPage } from './ScenarioPage'
import { Home } from './Home'
import { Roadmap } from './Roadmap'
import { TopBar } from '../components/TopBar'
import { QuizStrip } from '../components/QuizStrip'
import { mount, baseServer } from '../test/harness'
import { CATALOG } from '../dev/mockContent'
import { withFlowAndRoadmap, withQuiz } from '../dev/mockFlow'
import { quizActions, readQuiz } from '../lib/quiz'

const full = withQuiz(withFlowAndRoadmap(CATALOG, () => true))

beforeEach(() => {
  localStorage.clear()
  window.location.hash = ''
})

describe('a question for the room', () => {
  it('shows the situation and the choices, and nothing of the answer', async () => {
    mount(<Question id="skip-one-table" />, baseServer, full)
    expect(await screen.findByRole('heading', { level: 1, name: 'Wat doe je vannacht met de load?' })).toBeInTheDocument()
    expect(screen.getByText(/3 scans zonder scannummer/)).toBeInTheDocument()
    expect(screen.getAllByRole('listitem')).toHaveLength(3)
    expect(screen.queryByText(/Niemand weet nog welke cijfers/)).toBeNull()
    expect(screen.queryByText('Loopt goed')).toBeNull()
    expect(screen.queryByText('Sla het kapotte deel over, niet de hele nacht.')).toBeNull()
    expect(screen.queryByText('Bij ons gebeurd')).toBeNull()
  })

  it('says what really happened only once the answer is revealed', async () => {
    mount(<Question id="skip-one-table" />, baseServer, full)
    fireEvent.click(await screen.findByRole('button', { name: 'Toon de impact van elke keuze' }))
    const real = screen.getByRole('complementary', { name: 'Bij ons gebeurd' })
    expect(within(real).getByText(/werkte ze niet meer zoals bedoeld/)).toBeInTheDocument()
  })

  it('draws no such block for a question without one', async () => {
    mount(<Question id="late-delivery" />, baseServer, full)
    fireEvent.click(await screen.findByRole('button', { name: 'Toon de impact van elke keuze' }))
    expect(screen.getByText('Eén late feed houdt de rest niet tegen.')).toBeInTheDocument()
    expect(screen.queryByText('Bij ons gebeurd')).toBeNull()
  })

  it('counts a hand per tap or key, and one less with the minus button or Shift', async () => {
    mount(<Question id="skip-one-table" />, baseServer, full)
    const c = await screen.findByRole('button', { name: /Keuze C/ })
    fireEvent.click(c)
    fireEvent.click(c)
    fireEvent.keyDown(window, { key: 'a' })
    fireEvent.keyDown(window, { key: 'B' })
    fireEvent.keyDown(window, { key: 'B', shiftKey: true })
    fireEvent.click(screen.getByRole('button', { name: 'Eén hand minder voor C' }))
    expect(readQuiz().answers['skip-one-table']!.votes).toEqual([1, 0, 1])
    expect(screen.getByText('2 handen geteld.')).toBeInTheDocument()
  })

  it('reveals what every choice leads to, marks the choice of the platform and what the room chose', async () => {
    mount(<Question id="flow-test" step="try" />, baseServer, full)
    const b = await screen.findByRole('button', { name: /Keuze B/ })
    fireEvent.click(b)
    fireEvent.click(b)
    fireEvent.click(screen.getByRole('button', { name: /Keuze A/ }))
    fireEvent.click(screen.getByRole('button', { name: 'Toon de impact van elke keuze' }))
    const options = screen.getAllByRole('listitem')
    expect(options.map((o) => o.getAttribute('data-tone'))).toEqual(['warn', 'good', 'bad'])
    expect(within(options[1]!).getByText('Volgende stap')).toBeInTheDocument()
    expect(within(options[1]!).getByText('Private test copy')).toBeInTheDocument()
    expect(within(options[1]!).getByText('De keuze van de zaal')).toBeInTheDocument()
    expect(within(options[0]!).queryByText('Volgende stap')).toBeNull()
    expect(screen.getByText('De zaal koos wat het platform als volgende stap wil.')).toBeInTheDocument()
    expect(screen.getByText('Uitproberen hoort op een plek waar niemand anders leest.')).toBeInTheDocument()
    // counting is over once the impacts are shown
    expect(screen.queryByRole('button', { name: /Keuze A/ })).toBeNull()
    fireEvent.keyDown(window, { key: 'c' })
    expect(readQuiz().answers['flow-test/try']!.votes).toEqual([1, 2, 0])
  })

  it('reveals with Enter, and says the room is divided on a tie', async () => {
    mount(<Question id="skip-one-table" />, baseServer, full)
    await screen.findByRole('button', { name: /Keuze A/ })
    fireEvent.keyDown(window, { key: 'a' })
    fireEvent.keyDown(window, { key: 'c' })
    fireEvent.keyDown(window, { key: 'Enter' })
    expect(await screen.findByText('De zaal is verdeeld.')).toBeInTheDocument()
    expect(screen.getAllByText('Gedeeld eerste in de zaal')).toHaveLength(2)
    expect(screen.getByText('Dit toont de demo')).toBeInTheDocument()
  })

  it('leads to the proof and to the next question, and can be asked again', async () => {
    quizActions.reveal('skip-one-table')
    mount(<Question id="skip-one-table" />, baseServer, full)
    const proof = await screen.findByRole('link', { name: 'Bekijk het scenario' })
    expect(proof).toHaveAttribute('href', '#/s/skip-one-table/problem')
    expect(screen.getByRole('link', { name: 'Volgende vraag' })).toHaveAttribute('href', '#/quiz/flow-test/check')
    fireEvent.click(screen.getByRole('button', { name: 'Opnieuw vragen' }))
    expect(readQuiz().answers['skip-one-table']).toBeUndefined()
    expect(screen.getByRole('button', { name: 'Toon de impact van elke keuze' })).toBeInTheDocument()
  })

  it('points a question about a step at that step of the demo', async () => {
    quizActions.reveal('flow-test/release')
    mount(<Question id="flow-test" step="release" />, baseServer, full)
    expect(await screen.findByRole('link', { name: 'Bekijk het live' })).toHaveAttribute('href', '#/s/flow-test/demo?step=release')
  })

  it('lets the presenter get the demo ready while the room thinks', async () => {
    const { backend } = mount(<Question id="skip-one-table" />, baseServer, full)
    fireEvent.click(await screen.findByRole('button', { name: 'Demo alvast klaarzetten' }))
    expect(backend.prepare).toHaveBeenCalledWith('skip-one-table')
  })

  it('says so when there is no question here', async () => {
    mount(<Question id="switchover-date" />, baseServer, full)
    expect(await screen.findByRole('heading', { name: 'Hier hoort geen vraag bij' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Naar alle vragen' })).toHaveAttribute('href', '#/quiz')
  })
})

describe('the quiz page', () => {
  it('lists every question under its scenario, with the time it takes', async () => {
    mount(<QuizPage />, baseServer, full)
    expect(await screen.findByRole('heading', { level: 1, name: 'Wat zou jij doen?' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Wat doe je vannacht met de load?' })).toHaveAttribute('href', '#/quiz/skip-one-table')
    expect(screen.getByRole('link', { name: 'Waar probeer je de wijziging uit?' })).toHaveAttribute('href', '#/quiz/flow-test/try')
    expect(screen.getByText('Stap 4 van 6')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Nog geen route: alle 5 vragen tellen mee' })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Stel de eerste vraag' })).toHaveAttribute('href', '#/quiz/late-delivery')
  })

  it('adds up the picked route against the time of the session', async () => {
    mount(<QuizPage />, baseServer, full)
    fireEvent.click(await screen.findByRole('checkbox', { name: 'In de route: Wat doe je vannacht met de load?' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'In de route: Hoe breng je ze naar productie?' }))
    expect(screen.getByRole('heading', { name: 'Je route: 2 vragen' })).toBeInTheDocument()
    // 2 + 4 for the scenario, and 2 + five steps of 4 minutes for step 5 of the flow
    expect(screen.getByText('28 min')).toBeInTheDocument()
    expect(screen.getByText('92 min over.')).toBeInTheDocument()
    fireEvent.change(screen.getByRole('spinbutton'), { target: { value: '20' } })
    expect(screen.getByText('8 min te veel.')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Alles in de route' }))
    expect(readQuiz().plan).toHaveLength(4)
    fireEvent.click(screen.getByRole('button', { name: 'Route leegmaken' }))
    expect(readQuiz().plan).toEqual([])
  })

  it('shows what was played, and wipes the answers only after a second click', async () => {
    quizActions.vote('skip-one-table', 2, 1, 3)
    quizActions.reveal('skip-one-table')
    mount(<QuizPage />, baseServer, full)
    expect(await screen.findByText('A 0 · B 0 · C 1')).toBeInTheDocument()
    expect(screen.getByText(/De zaal koos 1 van de 1 keer wat het platform doet/)).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Stel de volgende vraag' })).toHaveAttribute('href', '#/quiz/late-delivery')
    fireEvent.click(screen.getByRole('button', { name: 'Nieuwe sessie: antwoorden en klok wissen' }))
    expect(readQuiz().answers['skip-one-table']).toBeDefined()
    fireEvent.click(screen.getByRole('button', { name: 'Zeker? Klik nog eens om alles te wissen' }))
    expect(readQuiz().answers).toEqual({})
  })

  it('says so when no scenario has a question', async () => {
    mount(<QuizPage />)
    expect(await screen.findByRole('heading', { name: 'Er zijn nog geen vragen' })).toBeInTheDocument()
  })
})

describe('the quiz inside the studio', () => {
  it('opens a scenario with its question first', async () => {
    const replace = vi.fn()
    vi.stubGlobal('location', { ...window.location, replace, hash: '' })
    try {
      mount(<ScenarioPage id="skip-one-table" part="problem" />, baseServer, full)
      await waitFor(() => expect(replace).toHaveBeenCalledWith('#/quiz/skip-one-table'))
    } finally {
      vi.unstubAllGlobals()
    }
  })

  const cases: [string, () => void, 'problem' | 'demo'][] = [
    ['the impacts of the question were shown', () => quizActions.reveal('skip-one-table'), 'problem'],
    ['the question was skipped', () => quizActions.skip('skip-one-table'), 'problem'],
    ['the presenter switched it off', () => quizActions.setOpening(false), 'problem'],
    ['another part is opened', () => undefined, 'demo'],
  ]
  it.each(cases)('opens the scenario itself when %s', async (_name, prepare, part) => {
    const replace = vi.fn()
    vi.stubGlobal('location', { ...window.location, replace, hash: '' })
    try {
      prepare()
      mount(<ScenarioPage id="skip-one-table" part={part} />, baseServer, full)
      expect(await screen.findByRole('heading', { level: 1 })).toHaveTextContent('One broken feed')
      expect(replace).not.toHaveBeenCalled()
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('keeps the question next to the story: a button to ask it, or what the room chose', async () => {
    const view = mount(<QuizStrip scenarioId="flow-test" stepId="try" />, baseServer, full)
    expect(await screen.findByRole('link', { name: 'Stel de vraag' })).toHaveAttribute('href', '#/quiz/flow-test/try')
    quizActions.vote('flow-test/try', 1, 1, 3)
    quizActions.reveal('flow-test/try')
    expect(await screen.findByText('De zaal koos: A 0 · B 1 · C 0')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: 'Bekijk de vraag' })).toBeInTheDocument()
    view.unmount()
    const none = mount(<QuizStrip scenarioId="flow-test" stepId="all" />, baseServer, full)
    await waitFor(() => expect(none.container.textContent).toBe(''))
  })

  it('has a way in from the home page and the top bar, only when there are questions', async () => {
    mount(
      <>
        <TopBar route={{ name: 'quiz' }} />
        <Home />
      </>,
      baseServer,
      full,
    )
    expect(await screen.findByRole('link', { name: 'Open de quiz' })).toHaveAttribute('href', '#/quiz')
    expect(screen.getByRole('link', { name: 'Quiz' })).toHaveAttribute('aria-current', 'page')
  })
  it('has no way in without questions', async () => {
    mount(
      <>
        <TopBar route={{ name: 'home' }} />
        <Home />
      </>,
    )
    await screen.findAllByRole('heading', { level: 2 })
    expect(screen.queryByRole('link', { name: 'Open de quiz' })).toBeNull()
    expect(screen.queryByRole('link', { name: 'Quiz' })).toBeNull()
  })

  it('closes the workshop with how often the room chose what the platform does', async () => {
    quizActions.vote('skip-one-table', 2, 1, 3)
    quizActions.reveal('skip-one-table')
    quizActions.vote('late-delivery', 1, 1, 2)
    quizActions.reveal('late-delivery')
    mount(<Roadmap />, baseServer, full)
    expect(await screen.findByText('De zaal koos 1 van de 2 keer wat het platform doet.')).toBeInTheDocument()
  })
})
