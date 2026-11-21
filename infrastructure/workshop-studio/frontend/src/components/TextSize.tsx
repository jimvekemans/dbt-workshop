import { useTextSize } from '../lib/textSize'
import { IconButton } from './IconButton'
import styles from './TextSize.module.css'

/** Two buttons that make all text of the app smaller or larger. The choice is remembered in this browser. */
export function TextSize() {
  const { size, canSmaller, canLarger, smaller, larger } = useTextSize()
  return (
    <div className={styles.group} role="group" aria-label="Tekstgrootte">
      <IconButton label="Kleinere tekst" disabled={!canSmaller} disabledReason="Dit is de kleinste tekstgrootte" onClick={smaller}>
        <span className={styles.small} aria-hidden="true">A</span>
      </IconButton>
      <IconButton label="Grotere tekst" disabled={!canLarger} disabledReason="Dit is de grootste tekstgrootte" onClick={larger}>
        <span className={styles.large} aria-hidden="true">A</span>
      </IconButton>
      <span className="sr-only" role="status" aria-live="polite">
        Tekstgrootte: {size.label}
      </span>
    </div>
  )
}
