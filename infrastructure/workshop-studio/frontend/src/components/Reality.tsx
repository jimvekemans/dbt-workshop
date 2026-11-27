import { cx } from '../lib/cx'
import { RichText } from './Term'
import styles from './Reality.module.css'

/** The heading of the block: what really happened at the client, next to the practice story of the webshop. */
export const REALITY_TITLE = 'Bij ons gebeurd'

interface Props {
  /** One or two plain sentences (`reality:` of a scenario or a stage). Nothing is drawn without it. */
  text?: string | null
  /** `lg` at the reveal of a question, `md` in part 1, `sm` inside a step. */
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

export function Reality({ text, size = 'md', className }: Props) {
  if (!text || !text.trim()) return null
  return (
    <aside className={cx(styles.reality, styles[size], className)} aria-label={REALITY_TITLE} data-reality>
      <p className={styles.label}>{REALITY_TITLE}</p>
      <p className={styles.text}>
        <RichText text={text} />
      </p>
    </aside>
  )
}
