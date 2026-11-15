import type { Scenario } from '../api/types'
import { Prose, RichText } from '../components/Term'
import { Callout } from '../components/Callout'
import { VisualView } from '../components/VisualView'
import { partTitle } from '../lib/router'
import styles from './parts.module.css'

export function ProblemPart({ scenario }: { scenario: Scenario }) {
  const p = scenario.problem
  return (
    <div className={styles.twoCol} data-part="problem">
      <div className={styles.areaHead}>
        <p className="kicker">{partTitle('problem')}</p>
        <h2 className={styles.h2} id="part-heading" tabIndex={-1}>
          <RichText text={p.headline} />
        </h2>
      </div>
      <Prose text={p.story} className={`${styles.story} ${styles.areaText}`} />
      <div className={styles.areaCost}>
        <Callout tone="bad" title="What it costs">
          <RichText text={p.cost} />
        </Callout>
      </div>
      <div className={`${styles.visual} ${styles.areaVisual}`}>
        <VisualView visual={p.visual} />
      </div>
    </div>
  )
}
