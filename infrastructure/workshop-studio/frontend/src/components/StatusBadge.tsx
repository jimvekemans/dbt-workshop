import type { FeatureStatus } from '../api/types'
import { cx } from '../lib/cx'
import { StatusMark } from './Status'
import styles from './StatusBadge.module.css'

/** The two phrases a status is shown with, everywhere (badges, counts, the closing overview). */
export const STATUS_WORDS: Record<FeatureStatus, string> = { today: 'Vandaag al in gebruik', next: 'Volgende stap' }

export function isFeatureStatus(x: unknown): x is FeatureStatus {
  return x === 'today' || x === 'next'
}

/**
 * "Vandaag al in gebruik" (calm green, a tick) or "Volgende stap" (blue, an arrow): shape, colour and words together.
 * Without a status (or with one it does not know) nothing is drawn.
 */
export function StatusBadge({ status, className }: { status?: FeatureStatus | string | null; className?: string }) {
  if (!isFeatureStatus(status)) return null
  return (
    <span className={cx(styles.badge, styles[status], className)} data-status={status}>
      {status === 'today' ? (
        <StatusMark state="ok" size="1em" />
      ) : (
        <svg className={styles.arrow} width="1em" height="1em" viewBox="0 0 20 20" aria-hidden="true" focusable="false">
          <circle cx="10" cy="10" r="9" fill="currentColor" />
          <path d="M5.5 10h8M10.5 6.2l3.8 3.8-3.8 3.8" fill="none" stroke="var(--surface)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
      )}
      <span>{STATUS_WORDS[status]}</span>
    </span>
  )
}
