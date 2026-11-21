import { useTechNames } from '../lib/techNames'
import styles from './TechNamesToggle.module.css'

/** The presenter's switch in the top bar: show or hide the small technical lines. On at the start; remembered in this browser. */
export function TechNamesToggle() {
  const { on, toggle } = useTechNames()
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label="Technische namen"
      title={on ? 'Technische namen zijn zichtbaar. Klik om ze te verbergen.' : 'Technische namen zijn verborgen. Klik om ze te tonen.'}
      className={styles.toggle}
      onClick={toggle}
    >
      <span className={styles.track} aria-hidden="true">
        <span className={styles.thumb} />
      </span>
      <span className={styles.long} aria-hidden="true">Technische namen</span>
      <span className={styles.short} aria-hidden="true">Tech</span>
    </button>
  )
}
