import type { CSSProperties } from 'react'
import { useStudio } from '../lib/studio'
import { flatScenarios } from '../lib/store'
import { StatusRow } from '../components/StatusRow'
import { ScenarioCard } from '../components/ScenarioCard'
import { SkeletonLines } from '../components/Skeleton'
import { Callout } from '../components/Callout'
import { RichText } from '../components/Term'
import { Button } from '../components/Button'
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
  let n = 0
  return (
    <div className={styles.page}>
      <header className={styles.hero}>
        <p className="kicker">A data team workshop</p>
        <h1 className={styles.h1}>Keeping reports right while everything around them changes</h1>
        <p className={styles.lead}>
          {total > 0 ? `${total} short stories` : 'Short stories'} about how a data team keeps company reports correct. Each has four parts: what goes wrong, the idea, the code, and a live demo. They are independent: start anywhere.
        </p>
        {pickNumber > 0 && (
          <p className={styles.hint}>
            Not sure? Start with {String(pickNumber).padStart(2, '0')}. Short on time? Every card has a link straight to its demo.
          </p>
        )}
      </header>

      <StatusRow />

      {!catalog && !catalogError && (
        <div className={styles.loading}>
          <SkeletonLines lines={5} label="Loading the scenarios" />
        </div>
      )}
      {catalogError && !catalog && connected && (
        <Callout tone="warn" title="The list of scenarios did not load" actions={<Button onClick={actions.retryCatalog}>Try again</Button>}>
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
    </div>
  )
}
