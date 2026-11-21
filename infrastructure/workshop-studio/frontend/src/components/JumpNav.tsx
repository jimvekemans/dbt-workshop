import { cx } from '../lib/cx'
import styles from './JumpNav.module.css'

export interface JumpItem {
  /** The id of the element to scroll to. */
  target: string
  title: string
  /** What the round mark shows: the number, or a tick / exclamation mark once the step is done / did not work. */
  mark: string
  state?: 'todo' | 'done' | 'failed' | 'running'
}

/**
 * A short map of a long page (a demo with many steps, a code part with many blocks): one button per part that scrolls
 * there and puts the keyboard focus on its heading. The buttons wrap onto as many rows as the width needs.
 */
export function JumpNav({ label, items }: { label: string; items: JumpItem[] }) {
  const go = (id: string) => {
    const el = document.getElementById(id)
    if (!el) return
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
    el.scrollIntoView?.({ behavior: reduce ? 'auto' : 'smooth', block: 'start' })
    el.focus?.({ preventScroll: true })
  }
  return (
    <nav aria-label={label} className={styles.nav}>
      <ol className={styles.list}>
        {items.map((it) => (
          <li key={it.target} className={styles.item}>
            <button type="button" className={cx(styles.jump, it.state && styles[it.state])} onClick={() => go(it.target)}>
              <span className={styles.mark} aria-hidden="true">{it.mark}</span>
              <span className={styles.title}>{it.title}</span>
              {it.state === 'done' && <span className="sr-only"> (klaar)</span>}
              {it.state === 'failed' && <span className="sr-only"> (mislukt)</span>}
            </button>
          </li>
        ))}
      </ol>
    </nav>
  )
}
