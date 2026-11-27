import type { CSSProperties } from 'react'
import { useStudio } from '../lib/studio'
import { flatScenarios } from '../lib/store'
import { quizItems } from '../lib/quiz'
import { StatusRow } from '../components/StatusRow'
import { ScenarioCard } from '../components/ScenarioCard'
import { SkeletonLines } from '../components/Skeleton'
import { Callout } from '../components/Callout'
import { RichText } from '../components/Term'
import { plainText } from '../lib/glossary'
import { Button } from '../components/Button'
import { LinkButton } from '../components/LinkButton'
import { ArrowRightIcon } from '../components/icons'
import styles from './Home.module.css'

/** Balanced rows on a wide screen, never one orphan card: 3 -> 3, 4 -> 2+2, 5 -> 3+2, 6 -> 3+3, 7 -> 4+3, 8 -> 4+4. */
export function columnsFor(n: number): number {
  if (n <= 3) return Math.max(1, n)
  if (n === 4) return 2
  if (n <= 6) return 3
  return 4
}

export function Home() {
  const { catalog, catalogError, connected, actions } = useStudio()
  const flat = flatScenarios(catalog)
  const total = flat.length
  // the order is the API's; the "good first pick" is the API's choice, or else the first scenario of the first group
  const pick = (catalog?.first_pick && flat.find((e) => e.id === catalog.first_pick)?.id) || flat.find((e) => e.first_pick)?.id || flat[0]?.id
  const pickNumber = flat.findIndex((e) => e.id === pick) + 1
  const questions = quizItems(catalog).length
  let n = 0
  return (
    <div className={styles.page}>
      <header className={styles.hero}>
        <p className="kicker">Een workshop voor datateams</p>
        <h1 className={styles.h1}>Rapporten correct houden terwijl alles errond verandert</h1>
        <p className={styles.lead}>
          {total > 0 ? `${total} korte verhalen` : 'Korte verhalen'} over hoe een datateam de rapporten van het bedrijf correct houdt. Elk verhaal heeft vier delen: wat er misloopt, het idee, de code en een live demo. Ze staan los van elkaar: begin waar je wil.
        </p>
        {pickNumber > 0 && (
          <p className={styles.hint}>
            Niet zeker? Begin met {String(pickNumber).padStart(2, '0')}. Weinig tijd? Elke kaart heeft een link rechtstreeks naar de demo.
          </p>
        )}
      </header>

      <StatusRow />

      {questions > 0 && (
        <section className={styles.quiz} aria-labelledby="quiz-h" data-quiz-card="">
          <div className={styles.quizText}>
            <p className="kicker">Samen met de zaal</p>
            <h2 id="quiz-h" className={styles.quizTitle}>
              Quiz: wat zou jij doen?
            </h2>
            <p>
              {questions} vragen, één bij elk scenario en bij elke stap van een flow. Eerst kiest de zaal, daarna toont de demo wat er echt gebeurt.
            </p>
          </div>
          <LinkButton variant="primary" size="lg" href="#/quiz" icon={<ArrowRightIcon />}>
            Open de quiz
          </LinkButton>
        </section>
      )}

      {!catalog && !catalogError && (
        <div className={styles.loading}>
          <SkeletonLines lines={5} label="Scenario's worden geladen" />
        </div>
      )}
      {catalogError && !catalog && connected && (
        <Callout tone="warn" title="De lijst met scenario's kon niet geladen worden" actions={<Button onClick={actions.retryCatalog}>Opnieuw proberen</Button>}>
          {catalogError}
        </Callout>
      )}

      {catalog?.groups.map((g) => (
        <section key={g.id} className={styles.group} aria-labelledby={`g-${g.id}`}>
          <h2 id={`g-${g.id}`} className={styles.groupTitle}>
            <RichText text={g.title} />
          </h2>
          {g.blurb && <p className={styles.blurb}><RichText text={g.blurb} /></p>}
          <ul className={styles.grid} style={{ '--cols': columnsFor(g.scenarios.length), ...(columnsFor(g.scenarios.length) >= 4 ? { '--card-title': 'var(--text-lg)' } : {}) } as CSSProperties}>
            {g.scenarios.map((s) => (
              <ScenarioCard key={s.id} s={s} number={++n} firstPick={s.id === pick} />
            ))}
          </ul>
        </section>
      ))}

      {catalog?.roadmap && (
        <section className={styles.closing} aria-labelledby="closing-h" data-closing-card="">
          <p className="kicker">Het einde van de workshop</p>
          <h2 id="closing-h" className={styles.closingTitle}>
            <RichText text={catalog.roadmap.title} />
          </h2>
          <p className={styles.closingText}>
            <RichText text={catalog.roadmap.intro} />
          </p>
          <div>
            <LinkButton variant="primary" size="lg" href="#/roadmap" icon={<ArrowRightIcon />} aria-label={`Open: ${plainText(catalog.roadmap.title)}`}>
              Bekijk het overzicht
            </LinkButton>
          </div>
        </section>
      )}
    </div>
  )
}
