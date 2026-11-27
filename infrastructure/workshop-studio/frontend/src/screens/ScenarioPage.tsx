import { useEffect, useRef } from 'react'
import type { PartId } from '../api/types'
import { useScenario, useStudio } from '../lib/studio'
import { useUi } from '../lib/ui'
import { demoFor, demoIsPreparing, findCatalogScenario, neighbours } from '../lib/store'
import { PARTS, navigate, partIndex, partTitle } from '../lib/router'
import { isWaiting, itemKey, questionHref, quizItems, useQuiz } from '../lib/quiz'
import { QuizStrip } from '../components/QuizStrip'
import { plainText } from '../lib/glossary'
import { RichText } from '../components/Term'
import { TechLine } from '../components/TechLine'
import { Stepper } from '../components/Stepper'
import { LinkButton } from '../components/LinkButton'
import { Callout } from '../components/Callout'
import { Button } from '../components/Button'
import { Related } from '../components/Related'
import { SkeletonLines } from '../components/Skeleton'
import { ArrowRightIcon, ChevronLeftIcon } from '../components/icons'
import { ProblemPart } from './ProblemPart'
import { IdeaPart } from './IdeaPart'
import { CodePart } from './CodePart'
import { DemoPart } from './DemoPart'
import styles from './ScenarioPage.module.css'
import parts from './parts.module.css'

/** Why the next part is worth opening, in a few plain words. */
const NEXT_HINT: Partial<Record<PartId, string>> = { idea: 'hoe je het voorkomt', code: 'hoe het eruitziet in het project', demo: 'kijk hoe het gebeurt' }

const BAR_PX = 60

export function ScenarioPage({ id, part, from, step }: { id: string; part: PartId; from?: string; step?: string }) {
  const { scenario, error, notFound } = useScenario(id)
  const { catalog, server, actions } = useStudio()
  const { openSwitcher } = useUi()
  const entry = findCatalogScenario(catalog, id)
  const origin = from ? findCatalogScenario(catalog, from) : null
  const idx = partIndex(part)
  const prevIdx = useRef(idx)
  const first = useRef(true)
  const dir = idx < prevIdx.current ? 'back' : 'forward'
  const ready = !!scenario
  // a scenario with a question for the room opens with that question (unless it was asked, skipped or switched off)
  const quiz = useQuiz()
  const opener = quizItems(catalog).find((i) => i.key === itemKey(id)) ?? null
  const askFirst = part === 'problem' && !from && !!opener && quiz.opening && isWaiting(quiz, opener.key)
  useEffect(() => {
    if (askFirst && opener) window.location.replace(questionHref(opener))
  }, [askFirst, opener])

  useEffect(() => {
    prevIdx.current = idx
    if (first.current) {
      first.current = false
      return
    }
    window.scrollTo({ top: 0 })
    document.getElementById('part-heading')?.focus({ preventScroll: true })
  }, [idx, id])

  useEffect(() => {
    document.title = scenario ? `${plainText(scenario.title)} · Workshop Studio` : 'Workshop Studio'
    return () => {
      document.title = 'Workshop Studio'
    }
  }, [scenario])

  // the Back / Next bar sticks to the bottom (one line, at most 60 px); things fixed to the bottom sit above it. It steps aside
  // (stays in the flow, right under the card) while it would cover the buttons of the demo's end card.
  const footerRef = useRef<HTMLElement>(null)
  useEffect(() => {
    if (part !== 'demo') return
    let raf = 0
    const check = () => {
      raf = 0
      const f = footerRef.current
      if (!f) return
      const card = document.querySelector('[data-end-card]')
      const r = card?.getBoundingClientRect()
      const covered = !!r && r.bottom > window.innerHeight - BAR_PX - 8 && r.top < window.innerHeight
      if (covered) f.setAttribute('data-clear', '')
      else f.removeAttribute('data-clear')
    }
    const on = () => {
      if (!raf) raf = requestAnimationFrame(check)
    }
    on()
    window.addEventListener('scroll', on, { passive: true })
    window.addEventListener('resize', on)
    const mo = new MutationObserver(on)
    mo.observe(document.body, { childList: true, subtree: true })
    return () => {
      window.removeEventListener('scroll', on)
      window.removeEventListener('resize', on)
      mo.disconnect()
      if (raf) cancelAnimationFrame(raf)
    }
  }, [part, ready])

  useEffect(() => {
    if (!ready) return
    document.documentElement.setAttribute('data-sticky-footer', '')
    return () => document.documentElement.removeAttribute('data-sticky-footer')
  }, [ready])

  if (notFound) {
    return (
      <div className={styles.missing}>
        <h1 className={styles.missingTitle}>Dit scenario bestaat niet</h1>
        <p>De link is misschien oud. Kies er een uit de lijst.</p>
        <div className={styles.missingActions}>
          <LinkButton href="#/" variant="primary">
            Terug naar alle scenario's
          </LinkButton>
          <Button onClick={openSwitcher}>Ga naar een scenario</Button>
        </div>
      </div>
    )
  }
  if (error) {
    return (
      <>
        <h1 className="sr-only">Scenario</h1>
      <Callout
        tone="bad"
        role="alert"
        title="Dit scenario kon niet geopend worden"
        actions={
          <>
            <Button onClick={() => actions.retryScenario(id)}>Opnieuw proberen</Button>
            <LinkButton href="#/" variant="quiet">
              Terug naar alle scenario's
            </LinkButton>
          </>
        }
      >
        {error}
      </Callout>
      </>
    )
  }
  if (!scenario || askFirst) {
    return (
      <div className={styles.loading}>
        <SkeletonLines lines={6} label="Scenario wordt geopend" />
      </div>
    )
  }

  const prev = PARTS[idx - 1]
  const next = PARTS[idx + 1]
  const nextScenario = neighbours(catalog, id).next
  const gettingReady = part === 'demo' && demoIsPreparing(demoFor(server, id), server)
  const go = (p: string) => navigate({ name: 'scenario', id, part: p as PartId })
  const href = (p: PartId) => `#/s/${encodeURIComponent(id)}/${p}`

  return (
    <div className={styles.page}>
      <header className={styles.top}>
        {origin && (
          <a className={styles.origin} href={`#/s/${encodeURIComponent(origin.id)}/problem`}>
            Terug naar: {plainText(origin.title)}
          </a>
        )}
        <p className="kicker">
          {entry?.groupTitle ?? 'Scenario'} · ongeveer {scenario.minutes} min, inclusief demo
        </p>
        <h1 className={styles.h1}><RichText text={scenario.title} /></h1>
        <TechLine tech={scenario.tech ?? entry?.tech} />
      </header>
      <Stepper parts={PARTS} current={part} onSelect={go} />
      <div className={parts.part} data-dir={dir} key={part}>
        {part === 'problem' && <QuizStrip scenarioId={id} />}
        {part === 'problem' && <ProblemPart scenario={scenario} />}
        {part === 'idea' && <IdeaPart scenario={scenario} />}
        {part === 'code' && <CodePart scenario={scenario} />}
        {part === 'demo' && <DemoPart scenario={scenario} focusStep={step} />}
      </div>
      {(part === 'idea' || part === 'demo') && !gettingReady && <Related items={scenario.related} fromId={id} />}
      <footer ref={footerRef} className={styles.footer}>
        {prev ? (
          <LinkButton href={href(prev.id)} variant="secondary" icon={<ChevronLeftIcon />} className={styles.back}>
            <span className={styles.long}>Terug: {partTitle(prev.id)}</span>
            <span className={styles.short}>Terug</span>
          </LinkButton>
        ) : (
          <LinkButton href="#/" variant="secondary" icon={<ChevronLeftIcon />} className={styles.back}>
            <span className={styles.long}>Terug naar alle scenario's</span>
            <span className={styles.short}>Alle scenario's</span>
          </LinkButton>
        )}
        {next ? (
          <LinkButton href={href(next.id)} variant="primary" icon={<ArrowRightIcon />} className={styles.next}>
            <span className={styles.long}>
              Volgende: {partTitle(next.id)}
              {NEXT_HINT[next.id] && <small> · {NEXT_HINT[next.id]}</small>}
            </span>
            <span className={styles.short}>Volgende</span>
          </LinkButton>
        ) : (
          !gettingReady && (
            <LinkButton href={nextScenario ? `#/s/${encodeURIComponent(nextScenario.id)}/problem` : '#/'} variant="primary" icon={<ArrowRightIcon />} className={styles.next}>
              <span className={styles.long}>{nextScenario ? `Volgende: ${plainText(nextScenario.title)}` : 'Kies een ander scenario'}</span>
              <span className={styles.short}>{nextScenario ? 'Volgend scenario' : "Alle scenario's"}</span>
            </LinkButton>
          )
        )}
      </footer>
    </div>
  )
}
