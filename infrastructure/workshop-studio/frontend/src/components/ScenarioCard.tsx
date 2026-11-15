import type { CatalogScenario } from '../api/types'
import { plainText } from '../lib/glossary'
import { RichText } from './Term'
import { Card } from './Card'
import { LinkButton } from './LinkButton'
import { Badge } from './Status'
import { ArrowRightIcon, ClockIcon } from './icons'
import styles from './ScenarioCard.module.css'

export function ScenarioCard({ s, number, firstPick }: { s: CatalogScenario; number: number; firstPick?: boolean }) {
  const base = `#/s/${encodeURIComponent(s.id)}`
  return (
    <Card as="li" className={styles.card}>
      <p className={styles.number} aria-hidden="true">
        {String(number).padStart(2, '0')}
        {firstPick && <span className={styles.pick}>Start here</span>}
      </p>
      <h3 className={styles.title}><RichText text={s.title} /></h3>
      <p className={styles.promise}><RichText text={s.promise} /></p>
      <p className={styles.meta}>
        <span className={styles.min}>
          <ClockIcon size="1em" /> About {s.minutes} min, including the demo
        </span>
        {!s.recorded && <Badge>Live only</Badge>}
      </p>
      <div className={styles.actions}>
        <LinkButton variant="primary" href={`${base}/problem`} icon={<ArrowRightIcon />} aria-label={`Start: ${plainText(s.title)}`}>
          Start
        </LinkButton>
        <a className={styles.jump} href={`${base}/demo`} aria-label={`Go straight to the demo: ${plainText(s.title)}`}>
          Go straight to the demo
        </a>
      </div>
    </Card>
  )
}
