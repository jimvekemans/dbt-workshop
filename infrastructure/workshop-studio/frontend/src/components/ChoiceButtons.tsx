import { useId } from 'react'
import type { ChoiceOption } from '../api/types'
import { cx } from '../lib/cx'
import styles from './ChoiceButtons.module.css'

export function normaliseOptions(options: (string | ChoiceOption)[]): ChoiceOption[] {
  return options.map((o) => (typeof o === 'string' ? { value: o, label: o } : o))
}

interface Props {
  label: string
  options: (string | ChoiceOption)[]
  value: string
  onChange: (v: string) => void
  help?: string
  disabled?: boolean
  /** Equal-width segments on a phone (a two-way switch). */
  segments?: boolean
  /** Options that cannot be picked right now (the reason is shown by the caller). */
  disabledValues?: string[]
}

/** Segmented buttons built from native radio inputs: arrow keys, focus and screen readers behave as users expect. */
export function ChoiceButtons({ label, options, value, onChange, help, disabled, disabledValues, segments }: Props) {
  const name = useId()
  const opts = normaliseOptions(options)
  return (
    <fieldset className={styles.field} disabled={disabled}>
      <legend className={styles.legend}>{label}</legend>
      <div className={cx(styles.group, segments && styles.segments)}>
        {opts.map((o) => (
          <label key={o.value} className={cx(styles.option, value === o.value && styles.selected)}>
            <input className={styles.input} type="radio" name={name} value={o.value} checked={value === o.value} disabled={disabledValues?.includes(o.value)} onChange={() => onChange(o.value)} />
            <span className={styles.check} aria-hidden="true">
              {value === o.value ? '✓' : ''}
            </span>
            <span>{o.label}</span>
          </label>
        ))}
      </div>
      {help && <p className={styles.help}>{help}</p>}
    </fieldset>
  )
}
