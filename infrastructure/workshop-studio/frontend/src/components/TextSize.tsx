import { useTextSize } from '../lib/textSize'
import { IconButton } from './IconButton'
import styles from './TextSize.module.css'

/** Two buttons that make all text of the app smaller or larger. The choice is remembered in this browser. */
export function TextSize() {
  const { size, canSmaller, canLarger, smaller, larger } = useTextSize()
  return (
    <div className={styles.group} role="group" aria-label="Text size">
      <IconButton label="Smaller text" disabled={!canSmaller} disabledReason="This is the smallest text size" onClick={smaller}>
        <span className={styles.small} aria-hidden="true">A</span>
      </IconButton>
      <IconButton label="Larger text" disabled={!canLarger} disabledReason="This is the largest text size" onClick={larger}>
        <span className={styles.large} aria-hidden="true">A</span>
      </IconButton>
      <span className="sr-only" role="status" aria-live="polite">
        Text size: {size.label}
      </span>
    </div>
  )
}
