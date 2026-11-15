import type { ReactNode } from 'react'
import { cx } from '../lib/cx'
import { formatWhen } from '../lib/format'
import { RichText } from './Term'
import { SkeletonLines } from './Skeleton'
import { StatusMark } from './Status'
import styles from './EvidenceFrame.module.css'

export type EvidencePhase = 'pending' | 'loading' | 'error' | 'ready'

interface Props {
  title: string
  caption?: string
  phase: EvidencePhase
  updating?: boolean
  error?: string | null
  size?: 'sm' | 'md' | 'lg'
  /** Replay: when the recording was made. */
  recordedAt?: string | null
  /** What the empty frame says: "Appears after you press the button above." or "Appears after step 2." */
  waitingText?: string
  /** A mail's caption is a reading guide: one line per hint, above the mail. */
  guide?: boolean
  children?: ReactNode
}

/**
 * One frame for every evidence view. It owns the loading, waiting, error and updating states and gives the
 * body a fixed minimum height, so refreshing data never moves what is below it.
 */
export function EvidenceFrame({ title, caption, phase, updating, error, size = 'md', recordedAt, waitingText, guide, children }: Props) {
  // a caption that states a result is only shown once there is something to show
  const shown = phase === 'ready' && caption ? caption : null
  const lines = guide && shown ? shown.split(/\n+/).map((l) => l.trim()).filter(Boolean) : []
  return (
    <section className={cx(styles.frame, styles[size], phase === 'pending' && styles.pending)} aria-busy={phase === 'loading' || updating || undefined} data-updating={updating ? '' : undefined}>
      <header className={styles.head}>
        <h4 className={styles.title}><RichText text={title} /></h4>
        {recordedAt && <span className={styles.chip}>Recorded {formatWhen(recordedAt)}</span>}
        {updating && (
          <span className={styles.live}>
            <StatusMark state="running" size="1em" /> Updating…
          </span>
        )}
      </header>
      {shown && lines.length > 1 ? (
        <ol className={styles.guide} aria-label="How to read this">
          {lines.map((l, i) => (
            <li key={i}>
              <RichText text={l} />
            </li>
          ))}
        </ol>
      ) : (
        shown && (
          <p className={styles.caption}>
            <RichText text={shown} />
          </p>
        )
      )}
      <div className={styles.body}>
        {phase === 'pending' && <p className={styles.note}>{waitingText ?? 'Appears after you press the button above.'}</p>}
        {phase === 'loading' && <SkeletonLines lines={4} label="Reading the latest information" />}
        {phase === 'error' && (
          <p className={styles.error} role="alert">
            <StatusMark state="bad" /> <span>{error || 'We could not read this just now.'}</span>
          </p>
        )}
        {phase === 'ready' && children}
      </div>
      <span className={styles.bar} aria-hidden="true" />
    </section>
  )
}

export function EmptyNote({ children }: { children: ReactNode }) {
  return <p className={styles.note}>{children}</p>
}
