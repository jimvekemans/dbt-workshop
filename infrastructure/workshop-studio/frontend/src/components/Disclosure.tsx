import type { ReactNode } from 'react'
import { ChevronDownIcon } from './icons'
import styles from './Disclosure.module.css'

/** Closed by default; native <details> gives keyboard and screen-reader behaviour for free. */
export function Disclosure({ summary, children, defaultOpen }: { summary: string; children: ReactNode; defaultOpen?: boolean }) {
  return (
    <details className={styles.disclosure} open={defaultOpen}>
      <summary className={styles.summary}>
        <ChevronDownIcon className={styles.chevron} />
        <span>{summary}</span>
      </summary>
      <div className={styles.body}>{children}</div>
    </details>
  )
}
