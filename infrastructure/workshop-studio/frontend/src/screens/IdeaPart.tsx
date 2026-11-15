import type { Scenario } from '../api/types'
import { RichText } from '../components/Term'
import { VisualView } from '../components/VisualView'
import { partTitle } from '../lib/router'
import styles from './parts.module.css'

export function IdeaPart({ scenario }: { scenario: Scenario }) {
  const p = scenario.idea
  return (
    <div className={styles.twoCol} data-part="idea">
      <div className={styles.areaHead}>
        <p className="kicker">{partTitle('idea')}</p>
        <h2 className={styles.h2} id="part-heading" tabIndex={-1}>
          <RichText text={p.headline} />
        </h2>
      </div>
      <ul className={`${styles.points} ${styles.areaText}`}>
        {p.points.map((t, i) => (
          <li key={i}>
            <span>
              <RichText text={t} />
            </span>
          </li>
        ))}
      </ul>
      <div className={`${styles.visual} ${styles.areaVisual}`}>
        <VisualView visual={p.visual} />
      </div>
    </div>
  )
}
