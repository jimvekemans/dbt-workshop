import { useId } from 'react'
import styles from './Toggle.module.css'

interface Props {
  label: string
  checked: boolean
  onChange: (v: boolean) => void
  help?: string
  disabled?: boolean
}

export function Toggle({ label, checked, onChange, help, disabled }: Props) {
  const id = useId()
  return (
    <div className={styles.wrap}>
      <label className={styles.row} htmlFor={id}>
        <input id={id} className={styles.input} type="checkbox" role="switch" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} aria-describedby={help ? `${id}-h` : undefined} />
        <span className={styles.track} aria-hidden="true">
          <span className={styles.thumb} />
        </span>
        <span className={styles.label}>{label}</span>
        <span className={styles.state} aria-hidden="true">{checked ? 'Aan' : 'Uit'}</span>
      </label>
      {help && (
        <p id={`${id}-h`} className={styles.help}>
          {help}
        </p>
      )}
    </div>
  )
}
