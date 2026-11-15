import type { ReactNode } from 'react'
import { cx } from '../lib/cx'
import styles from './Status.module.css'

export type StatusState = 'ok' | 'bad' | 'warn' | 'idle' | 'running'

const DEFAULT_LABEL: Record<StatusState, string> = {
  ok: 'Fine',
  bad: 'Problem',
  warn: 'Careful',
  idle: 'Waiting',
  running: 'Working',
}

/** Status is always shape + colour + words. The shape differs per state so it survives greyscale. */
export function StatusMark({ state, size = '1.15em' }: { state: StatusState; size?: string }) {
  return (
    <svg className={cx(styles.mark, styles[state])} width={size} height={size} viewBox="0 0 20 20" aria-hidden="true" focusable="false">
      {state === 'ok' && (
        <>
          <circle cx="10" cy="10" r="9" fill="currentColor" />
          <path d="M5.5 10.3l3 3 6-6.4" fill="none" stroke="var(--mark-ink)" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
        </>
      )}
      {state === 'bad' && (
        <>
          <path d="M6.2 1.5h7.6L18.5 6.2v7.6l-4.7 4.7H6.2L1.5 13.8V6.2z" fill="currentColor" />
          <path d="M7 7l6 6M13 7l-6 6" stroke="var(--mark-ink)" strokeWidth="2.2" strokeLinecap="round" />
        </>
      )}
      {state === 'warn' && (
        <>
          <path d="M10 1.8l8.6 15.2H1.4z" fill="currentColor" strokeLinejoin="round" stroke="currentColor" strokeWidth="1.6" />
          <path d="M10 7.5v5M10 14.6v.1" stroke="var(--mark-ink)" strokeWidth="2.2" strokeLinecap="round" />
        </>
      )}
      {state === 'idle' && <circle cx="10" cy="10" r="7.8" fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray="3.2 3.2" />}
      {state === 'running' && (
        <g className={styles.spin}>
          <circle cx="10" cy="10" r="7.8" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="2.4" />
          <path d="M10 2.2a7.8 7.8 0 017.8 7.8" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" />
        </g>
      )}
    </svg>
  )
}

interface StatusProps {
  state: StatusState
  children?: ReactNode
  /** 'pill' has a tinted background; 'plain' is just the mark and the words. */
  variant?: 'pill' | 'plain'
  className?: string
}

export function Status({ state, children, variant = 'pill', className }: StatusProps) {
  return (
    <span className={cx(styles.status, styles[state], variant === 'pill' && styles.pill, className)}>
      <StatusMark state={state} />
      <span>{children ?? DEFAULT_LABEL[state]}</span>
    </span>
  )
}

export function Badge({ children, tone = 'neutral', className }: { children: ReactNode; tone?: 'neutral' | 'accent'; className?: string }) {
  return <span className={cx(styles.badge, tone === 'accent' && styles.accent, className)}>{children}</span>
}
