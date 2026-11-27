import { useEffect, useMemo, useState } from 'react'
import { useStudio } from '../lib/studio'
import { isPlayed, nextItem, questionHref, quizActions, quizItems, routeMinutes, score, tally, timing, useQuiz, votesText } from '../lib/quiz'
import type { QuizItem, QuizState } from '../lib/quiz'
import { plainText } from '../lib/glossary'
import { useNow } from '../lib/useNow'
import { cx } from '../lib/cx'
import { RichText } from '../components/Term'
import { Button } from '../components/Button'
import { LinkButton } from '../components/LinkButton'
import { Callout } from '../components/Callout'
import { Toggle } from '../components/Toggle'
import { StatusMark } from '../components/Status'
import { SkeletonLines } from '../components/Skeleton'
import { ArrowRightIcon, ChevronLeftIcon, ClockIcon } from '../components/icons'
import styles from './QuizPage.module.css'

interface Block {
  scenarioId: string
  title: string
  flow: boolean
  items: QuizItem[]
}
interface Group {
  id: string
  title: string
  blocks: Block[]
}

/** The questions as the page lists them: per group, per scenario, in the order of the home page. */
export function groupQuestions(items: QuizItem[]): Group[] {
  const groups: Group[] = []
  for (const item of items) {
    let g = groups.find((x) => x.id === item.groupId)
    if (!g) groups.push((g = { id: item.groupId, title: item.groupTitle, blocks: [] }))
    let b = g.blocks.find((x) => x.scenarioId === item.scenarioId)
    if (!b) g.blocks.push((b = { scenarioId: item.scenarioId, title: item.scenarioTitle, flow: item.kind === 'flow', items: [] }))
    b.items.push(item)
  }
  return groups
}

function Row({ item, state }: { item: QuizItem; state: QuizState }) {
  const picked = state.plan.includes(item.key)
  const answer = state.answers[item.key]
  const played = isPlayed(state, item.key)
  const t = tally(item.quiz, answer)
  const question = plainText(item.quiz.question)
  return (
    <li className={cx(styles.row, picked && styles.picked, played && styles.played)}>
      <label className={styles.check}>
        <input type="checkbox" checked={picked} onChange={(e) => quizActions.setPlanned([item.key], e.target.checked)} aria-label={`In de route: ${question}`} />
      </label>
      <span className={styles.what}>
        <a className={styles.link} href={questionHref(item)}>
          {question}
        </a>
        {item.stepIndex !== null && <span className={styles.stage}>Stap {item.stepIndex + 1} van {item.stepCount}</span>}
      </span>
      <span className={styles.state}>
        {played ? (
          <>
            <StatusMark state="ok" size="1em" />
            <span>{t.total > 0 ? votesText(t) : 'Gespeeld'}</span>
          </>
        ) : answer?.skipped ? (
          <span>Overgeslagen</span>
        ) : null}
      </span>
      <span className={styles.min} title={`${item.talk} min vraag, ${item.demo} min demo`}>
        {item.talk} + {item.demo} min
      </span>
    </li>
  )
}

/** The quiz: every question for the room, the route the presenter picked and the time that route takes. */
export function QuizPage() {
  const { catalog, catalogError, connected, actions } = useStudio()
  const state = useQuiz()
  const items = useMemo(() => quizItems(catalog), [catalog])
  const groups = useMemo(() => groupQuestions(items), [items])
  const running = state.startedAt !== null
  const now = useNow(running, 15000)
  const [sure, setSure] = useState(false)

  useEffect(() => {
    document.title = 'Quiz · Workshop Studio'
    return () => {
      document.title = 'Workshop Studio'
    }
  }, [])

  const ready = !!catalog
  useEffect(() => {
    if (!ready) return
    window.scrollTo?.({ top: 0 })
    document.getElementById('part-heading')?.focus({ preventScroll: true })
  }, [ready])

  useEffect(() => {
    if (!sure) return
    const t = setTimeout(() => setSure(false), 4000)
    return () => clearTimeout(t)
  }, [sure])

  if (!catalog) {
    return (
      <div className={styles.page}>
        {catalogError && connected ? (
          <Callout tone="warn" title="De quiz kon niet geladen worden" actions={<Button onClick={actions.retryCatalog}>Opnieuw proberen</Button>}>
            {catalogError}
          </Callout>
        ) : (
          <SkeletonLines lines={5} label="Quiz wordt geladen" />
        )}
      </div>
    )
  }

  const back = (
    <LinkButton variant="quiet" href="#/" icon={<ChevronLeftIcon />}>
      Terug naar alle scenario's
    </LinkButton>
  )

  if (items.length === 0) {
    return (
      <div className={styles.page}>
        <h1 className={styles.h1}>Er zijn nog geen vragen</h1>
        <p className={styles.lead}>Niemand schreef al een vraag bij een scenario van deze workshop.</p>
        <div>{back}</div>
      </div>
    )
  }

  const time = timing(items, state, now)
  const result = score(items, state)
  const next = nextItem(items, state)
  const answered = Object.keys(state.answers).length > 0
  const share = Math.min(1, time.plannedMinutes / Math.max(1, time.budget))

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <p className="kicker">Quiz</p>
        <h1 className={styles.h1} id="part-heading" tabIndex={-1}>
          Wat zou jij doen?
        </h1>
        <p className={styles.lead}>
          Bij elk scenario en bij elke stap van een flow hoort een vraag. Eerst kiest de zaal, dan zie je wat elke keuze kost, en daarna toont de demo wat er echt gebeurt.
        </p>
      </header>

      <section className={styles.panel} aria-labelledby="quiz-route">
        <div className={styles.now}>
          <h2 id="quiz-route" className={styles.panelTitle}>
            {time.picked ? `Je route: ${time.questions} ${time.questions === 1 ? 'vraag' : 'vragen'}` : `Nog geen route: alle ${time.questions} vragen tellen mee`}
          </h2>
          <p className={styles.sum}>
            <b>{time.plannedMinutes} min</b> gepland van de {time.budget} min, vraag en demo samen.{' '}
            {time.elapsed === null ? (
              <span className={cx(styles.slack, time.slack < 0 && styles.over)}>{time.slack >= 0 ? `${time.slack} min over.` : `${-time.slack} min te veel.`}</span>
            ) : (
              <span className={cx(styles.slack, time.slack < 0 && styles.over)}>
                {time.elapsed} min bezig, nog {time.toGoMinutes} min te spelen: {time.slack >= 0 ? `${time.slack} min marge.` : `${-time.slack} min te kort.`}
              </span>
            )}
          </p>
          <span className={styles.meter} aria-hidden="true">
            <span className={cx(styles.meterFill, time.plannedMinutes > time.budget && styles.meterOver)} style={{ width: `${Math.round(share * 100)}%` }} />
          </span>
          {result.played > 0 && (
            <p className={styles.score}>
              {result.played} gespeeld.{' '}
              {result.counted > 0 && `De zaal koos ${result.agreed} van de ${result.counted} keer wat het platform doet.`}
            </p>
          )}
        </div>
        <div className={styles.controls}>
          {next ? (
            <LinkButton variant="primary" size="lg" href={questionHref(next)} icon={<ArrowRightIcon />}>
              {result.played > 0 ? 'Stel de volgende vraag' : 'Stel de eerste vraag'}
            </LinkButton>
          ) : (
            <LinkButton variant="primary" size="lg" href={catalog.roadmap ? '#/roadmap' : '#/'} icon={<ArrowRightIcon />}>
              {catalog.roadmap ? 'Alles gespeeld: naar het slotoverzicht' : 'Alles gespeeld'}
            </LinkButton>
          )}
          <label className={styles.budget}>
            <span>Tijd voor de sessie</span>
            <input type="number" min={10} max={600} step={5} value={state.budget} onChange={(e) => e.target.value && quizActions.setBudget(Number(e.target.value))} />
            <span>min</span>
          </label>
          {running ? (
            <Button icon={<ClockIcon />} onClick={() => quizActions.stopClock()}>
              Klok stoppen
            </Button>
          ) : (
            <Button icon={<ClockIcon />} onClick={() => quizActions.startClock()}>
              Klok starten
            </Button>
          )}
        </div>
      </section>

      {groups.map((g) => (
        <section key={g.id} className={styles.group} aria-labelledby={`qg-${g.id}`}>
          <h2 id={`qg-${g.id}`} className={styles.groupTitle}>
            <RichText text={g.title} />
          </h2>
          {g.blocks.map((b) => {
            const keys = b.items.map((i) => i.key)
            const all = keys.every((k) => state.plan.includes(k))
            const minutes = routeMinutes(b.items, state)
            return (
              <div key={b.scenarioId} className={styles.block}>
                <div className={styles.blockHead}>
                  <h3 className={styles.blockTitle}>
                    <a className={styles.scenario} href={`#/s/${encodeURIComponent(b.scenarioId)}/problem`}>
                      {plainText(b.title)}
                    </a>
                    {b.flow && (
                      <span className={styles.flow} title="De stappen van een flow lopen in volgorde: een vraag over een latere stap toont ook de demo van de stappen ervoor.">
                        Flow · {minutes} min voor alle vragen · stappen lopen in volgorde
                      </span>
                    )}
                  </h3>
                  {b.items.length > 1 && (
                    <Button variant="quiet" onClick={() => quizActions.setPlanned(keys, !all)}>
                      {all ? 'Alles uit de route' : 'Alles in de route'}
                    </Button>
                  )}
                </div>
                <ul className={styles.rows}>
                  {b.items.map((i) => (
                    <Row key={i.key} item={i} state={state} />
                  ))}
                </ul>
              </div>
            )
          })}
        </section>
      ))}

      <section className={styles.settings} aria-labelledby="quiz-settings">
        <h2 id="quiz-settings" className={styles.settingsTitle}>
          Instellingen van de quiz
        </h2>
        <Toggle
          label="Een scenario opent met zijn vraag"
          checked={state.opening}
          onChange={(v) => quizActions.setOpening(v)}
          help="Aan: wie een scenario opent, krijgt eerst de vraag aan de zaal. Uit: het scenario begint meteen bij deel 1."
        />
        <div className={styles.settingsActions}>
          <Button
            variant={sure ? 'danger' : 'secondary'}
            disabled={!answered && !running}
            disabledReason="Er is nog niets geantwoord."
            onClick={() => {
              if (!sure) return setSure(true)
              quizActions.clearAnswers()
              setSure(false)
            }}
          >
            {sure ? 'Zeker? Klik nog eens om alles te wissen' : 'Nieuwe sessie: antwoorden en klok wissen'}
          </Button>
          <Button disabled={state.plan.length === 0} disabledReason="Er zit nog niets in de route." onClick={() => quizActions.clearPlan()}>
            Route leegmaken
          </Button>
        </div>
        <p className={styles.note}>De route, de antwoorden en de klok blijven enkel in deze browser bewaard.</p>
      </section>

      <div>{back}</div>
    </div>
  )
}
