import { useEffect, useMemo } from 'react'
import type { QuizTone } from '../api/types'
import { useStudio } from '../lib/studio'
import { demoFor, demoIsOpen, demoIsPreparing, findCatalogScenario, stepState } from '../lib/store'
import { LETTERS, itemKey, nextItem, proofHref, questionHref, quizActions, quizItems, routeItems, tally, timing, useQuiz } from '../lib/quiz'
import type { QuizItem, Tally } from '../lib/quiz'
import { plainText } from '../lib/glossary'
import { useNow } from '../lib/useNow'
import { cx } from '../lib/cx'
import { RichText } from '../components/Term'
import { Button } from '../components/Button'
import { LinkButton } from '../components/LinkButton'
import { Callout } from '../components/Callout'
import { Reality } from '../components/Reality'
import { StatusMark } from '../components/Status'
import { StatusBadge } from '../components/StatusBadge'
import { SkeletonLines } from '../components/Skeleton'
import { ArrowRightIcon, ChevronLeftIcon, PlayIcon, RefreshIcon } from '../components/icons'
import styles from './Question.module.css'

const TONE_WORDS: Record<QuizTone, string> = { good: 'Loopt goed', warn: 'Heeft een prijs', bad: 'Loopt mis' }
const TONE_MARK = { good: 'ok', warn: 'warn', bad: 'bad' } as const

/** One sentence on how the room's hands compare with the choice the platform makes. */
export function verdict(item: QuizItem, t: Tally): string | null {
  if (t.total === 0) return null
  if (t.agrees) return item.status === 'next' ? 'De zaal koos wat het platform als volgende stap wil.' : item.status === 'today' ? 'De zaal koos wat het platform vandaag doet.' : 'De zaal koos wat de demo toont.'
  if (t.top.length > 1) return 'De zaal is verdeeld.'
  return 'De zaal koos een andere weg dan het platform.'
}

const hands = (n: number) => (n === 1 ? 'hand' : 'handen')

function typing(t: EventTarget | null): boolean {
  return t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName))
}
function pressable(t: EventTarget | null): boolean {
  return t instanceof HTMLElement && !!t.closest('button, a, [role="switch"], summary')
}

/** A question for the room: the situation, the choices with a count of raised hands, then what each choice leads to. */
export function Question({ id, step }: { id: string; step?: string }) {
  const { catalog, catalogError, connected, server, actions } = useStudio()
  const state = useQuiz()
  const items = useMemo(() => quizItems(catalog), [catalog])
  const key = itemKey(id, step)
  const item = items.find((i) => i.key === key) ?? null
  const answer = state.answers[key]
  const revealed = answer?.revealed === true
  const count = item?.quiz.options.length ?? 0
  const now = useNow(state.startedAt !== null, 15000)

  useEffect(() => {
    document.title = item ? `${plainText(item.quiz.question)} · Workshop Studio` : 'Workshop Studio'
    return () => {
      document.title = 'Workshop Studio'
    }
  }, [item])

  const ready = !!item
  useEffect(() => {
    if (!ready) return
    window.scrollTo?.({ top: 0 })
    document.getElementById('part-heading')?.focus({ preventScroll: true })
  }, [ready, key])

  // the impacts are what the room reads next: show them from the top of the page
  useEffect(() => {
    if (revealed) window.scrollTo?.({ top: 0 })
  }, [revealed])

  // A, B, C count a hand (with Shift: one less); Enter shows the impact
  useEffect(() => {
    if (!ready) return
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return
      if (document.querySelector('[role="dialog"][aria-modal="true"]')) return
      if (revealed) return
      const i = LETTERS.findIndex((l) => l.toLowerCase() === e.key.toLowerCase())
      if (i >= 0 && i < count) {
        e.preventDefault()
        quizActions.vote(key, i, e.shiftKey ? -1 : 1, count)
      } else if (e.key === 'Enter' && !pressable(e.target)) {
        e.preventDefault()
        quizActions.reveal(key)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [ready, key, count, revealed])

  if (!catalog) {
    return (
      <div className={styles.page}>
        {catalogError && connected ? (
          <Callout tone="warn" title="De vraag kon niet geladen worden" actions={<Button onClick={actions.retryCatalog}>Opnieuw proberen</Button>}>
            {catalogError}
          </Callout>
        ) : (
          <SkeletonLines lines={5} label="Vraag wordt geladen" />
        )}
      </div>
    )
  }
  if (!item) {
    return (
      <div className={styles.page}>
        <h1 className={styles.missing}>Hier hoort geen vraag bij</h1>
        <p className={styles.situation}>De link is misschien oud. Kies een vraag uit de lijst.</p>
        <div className={styles.actions}>
          <LinkButton variant="primary" href="#/quiz">
            Naar alle vragen
          </LinkButton>
          <LinkButton variant="quiet" href="#/">
            Terug naar alle scenario's
          </LinkButton>
        </div>
      </div>
    )
  }

  const q = item.quiz
  const t = tally(q, answer)
  const route = routeItems(items, state)
  const place = route.findIndex((i) => i.key === key)
  const next = nextItem(items, state, key)
  const time = timing(items, state, now)
  const demo = demoFor(server, id)
  const demoOpen = demoIsOpen(demo)
  const demoBusy = demoIsPreparing(demo, server)
  // the steps of a demo run in order: say so when the step of this question still waits for an earlier one
  const before = item.stepIndex ? findCatalogScenario(catalog, id)?.steps?.[item.stepIndex - 1] : undefined
  const waits = !!before && stepState(demo, before.id)?.status !== 'done'
  const said = verdict(item, t)
  const proof = item.stepId ? 'Bekijk het live' : 'Bekijk het scenario'

  return (
    <div className={styles.page} data-revealed={revealed || undefined}>
      <header className={styles.head}>
        <a className={styles.back} href="#/quiz">
          <ChevronLeftIcon size="1em" /> Alle vragen
        </a>
        <p className="kicker">
          Vraag aan de zaal{place >= 0 && ` · ${place + 1} van ${route.length}`} · <RichText text={item.groupTitle} />
        </p>
        {time.elapsed !== null && (
          <p className={cx(styles.clock, time.slack < 0 && styles.late)}>
            {time.elapsed} min bezig · {time.slack >= 0 ? `${time.slack} min marge` : `${-time.slack} min te kort`}
          </p>
        )}
      </header>

      <p className={styles.situation}>
        <RichText text={q.situation} />
      </p>
      <h1 className={styles.question} id="part-heading" tabIndex={-1}>
        <RichText text={q.question} />
      </h1>

      <ol className={styles.options} data-count={count}>
        {q.options.map((o, i) => {
          const votes = t.votes[i] ?? 0
          const letter = LETTERS[i]
          const top = t.top.includes(i)
          const share = t.total > 0 ? votes / t.total : 0
          const body = (
            <>
              <span className={styles.letter} aria-hidden="true">{letter}</span>
              <span className={styles.label}>
                <span className="sr-only">Keuze {letter}: </span>
                {plainText(o.label)}
              </span>
              <span className={styles.votes} aria-label={`${votes} ${hands(votes)}`}>
                <b>{votes}</b>
                <small aria-hidden="true">{hands(votes)}</small>
              </span>
            </>
          )
          return (
            <li key={i} className={cx(styles.option, revealed && styles[o.tone], revealed && o.platform && styles.platform)} data-tone={revealed ? o.tone : undefined}>
              {revealed ? (
                <div className={styles.pick}>{body}</div>
              ) : (
                <button type="button" className={styles.pick} onClick={() => quizActions.vote(key, i, 1, count)} aria-keyshortcuts={letter} title={`Eén hand erbij voor ${letter} (toets ${letter})`}>
                  {body}
                </button>
              )}
              {!revealed && (
                <button type="button" className={styles.minus} onClick={() => quizActions.vote(key, i, -1, count)} aria-label={`Eén hand minder voor ${letter}`} aria-disabled={votes === 0 || undefined} title={`Eén hand minder voor ${letter} (Shift + ${letter})`}>
                  −
                </button>
              )}
              {revealed && (
                <div className={styles.result}>
                  {t.total > 0 && (
                    <span className={styles.bar} aria-hidden="true">
                      <span className={styles.fill} style={{ width: `${Math.round(share * 100)}%` }} />
                    </span>
                  )}
                  <p className={styles.tone}>
                    <StatusMark state={TONE_MARK[o.tone]} size="1.2em" />
                    <span>{TONE_WORDS[o.tone]}</span>
                    {top && <span className={styles.room}>{t.top.length > 1 ? 'Gedeeld eerste in de zaal' : 'De keuze van de zaal'}</span>}
                  </p>
                  <p className={styles.impact}>
                    <RichText text={o.impact} />
                  </p>
                  {o.platform && (
                    <p className={styles.ours}>
                      {item.status ? <StatusBadge status={item.status} /> : <span className={styles.shown}>Dit toont de demo</span>}
                      {item.feature && <span className={styles.feature}>{plainText(item.feature)}</span>}
                    </p>
                  )}
                </div>
              )}
            </li>
          )
        })}
      </ol>

      {revealed && <Reality size="lg" text={item.reality} />}

      {!revealed ? (
        <div className={styles.actions}>
          <Button variant="primary" size="lg" onClick={() => quizActions.reveal(key)} aria-keyshortcuts="Enter">
            Toon de impact van elke keuze
          </Button>
          <p className={styles.hint} aria-live="polite">
            {t.total === 0 ? 'Tik op een keuze per opgestoken hand, of gebruik de toetsen A, B en C.' : `${t.total} ${hands(t.total)} geteld.`}
          </p>
          <a className={styles.skip} href={proofHref(item)} onClick={() => quizActions.skip(key)}>
            Overslaan
          </a>
        </div>
      ) : (
        <div className={styles.end}>
          {(said || q.takeaway) && (
            <div className={styles.after} role="status">
              {said && <p className={styles.verdict}>{said}</p>}
              {q.takeaway && (
                <p className={styles.takeaway}>
                  <RichText text={q.takeaway} />
                </p>
              )}
            </div>
          )}
          <div className={styles.actions}>
            <LinkButton variant="primary" size="lg" href={proofHref(item)} icon={<PlayIcon size="0.9em" />}>
              {proof}
            </LinkButton>
            {next ? (
              <LinkButton size="lg" href={questionHref(next)} icon={<ArrowRightIcon />}>
                Volgende vraag
              </LinkButton>
            ) : (
              <LinkButton size="lg" href={catalog.roadmap ? '#/roadmap' : '#/quiz'} icon={<ArrowRightIcon />}>
                {catalog.roadmap ? 'Naar het slotoverzicht' : 'Naar alle vragen'}
              </LinkButton>
            )}
            <Button variant="quiet" icon={<RefreshIcon />} onClick={() => quizActions.again(key)}>
              Opnieuw vragen
            </Button>
          </div>
        </div>
      )}

      <aside className={styles.presenter} aria-label="Voor de presentator">
        <span className={styles.presenterLabel}>Voor de presentator</span>
        {demoOpen ? (
          <span>De demo van dit scenario staat klaar.{waits && ` Deze stap kan pas lopen als stap ${item.stepIndex} klaar is.`}</span>
        ) : demoBusy ? (
          <span>De demo wordt klaargezet terwijl de zaal nadenkt.</span>
        ) : (
          <>
            <span>De demo staat nog niet klaar.</span>
            <Button disabled={!server || !!server.job} disabledReason={server?.job ? `Wacht tot dit klaar is: ${server.job.label}` : 'Wachten tot de Studio antwoordt.'} onClick={() => void actions.prepare(id)}>
              Demo alvast klaarzetten
            </Button>
          </>
        )}
      </aside>
    </div>
  )
}
