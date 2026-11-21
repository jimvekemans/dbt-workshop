import { cx } from '../lib/cx'
import { techText, useTechNames } from '../lib/techNames'
import styles from './TechLine.module.css'

/**
 * The technical line for the presenter: small, muted, monospace, clearly second to the plain title. Keywords of a
 * scenario are joined with " · "; a step has one line of its own. Nothing is drawn without `tech` or when the
 * presenter has switched "Technical names" off.
 */
export function TechLine({ tech, as: Tag = 'p', className }: { tech?: readonly string[] | string | null; as?: 'p' | 'span' | 'div'; className?: string }) {
  const { on } = useTechNames()
  const text = techText(tech)
  if (!on || !text) return null
  return (
    <Tag className={cx(styles.tech, className)} data-tech="">
      {text}
    </Tag>
  )
}
