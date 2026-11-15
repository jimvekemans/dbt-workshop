import { useId } from 'react'
import type { CSSProperties, KeyboardEvent, PointerEvent } from 'react'
import styles from './Slider.module.css'

interface Props {
  label: string
  value: number
  min: number
  max: number
  step?: number
  onChange: (v: number) => void
  /** Visible value, e.g. "3 days". */
  display?: string
  /** Spoken value for screen readers when it differs from the number. */
  valueText?: string
  help?: string
  disabled?: boolean
  minLabel?: string
  maxLabel?: string
  hideLabel?: boolean
  /** A tick on the track: `at` is 0 to 1 along the track, `label` says what it is. */
  mark?: { at: number; label: string }
  /** Small ticks with a label under the track (every day, for example). */
  ticks?: { at: number; label: string }[]
}

export function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v))
}

/** A native range input (touch, pointer and screen-reader behaviour for free) with explicit, testable keys. */
const THUMB_REM = 1.7
/** Where along the track (0 to 1) a pointer is, leaving room for half a thumb at each end. */
export function trackFraction(clientX: number, left: number, width: number, rootPx: number): number {
  const thumb = THUMB_REM * rootPx
  if (width <= thumb) return 0
  return clamp((clientX - left - thumb / 2) / (width - thumb), 0, 1)
}

export function Slider({ label, value, min, max, step = 1, onChange, display, valueText, help, disabled, minLabel, maxLabel, hideLabel, mark, ticks }: Props) {
  const id = useId()
  const pct = max === min ? 0 : ((value - min) / (max - min)) * 100
  const set = (v: number) => {
    const snapped = Math.round((v - min) / step) * step + min
    onChange(clamp(snapped, min, max))
  }
  // a plain click anywhere on the track moves the thumb there (then a drag carries on natively)
  const onPointerDown = (e: PointerEvent<HTMLInputElement>) => {
    if (disabled || e.button > 0) return
    const r = e.currentTarget.getBoundingClientRect()
    const root = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
    set(min + trackFraction(e.clientX, r.left, r.width, root) * (max - min))
  }
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (disabled) return
    const big = Math.max(step, Math.round((max - min) / 10 / step) * step)
    const map: Record<string, number> = {
      ArrowRight: value + step,
      ArrowUp: value + step,
      ArrowLeft: value - step,
      ArrowDown: value - step,
      PageUp: value + big,
      PageDown: value - big,
      Home: min,
      End: max,
    }
    const next = map[e.key]
    if (next !== undefined) {
      e.preventDefault()
      set(next)
    }
  }
  return (
    <div className={styles.slider}>
      <div className={styles.head}>
        <label htmlFor={id} className={hideLabel ? 'sr-only' : styles.label}>
          {label}
        </label>
        {display && <output htmlFor={id} className={styles.value}>{display}</output>}
      </div>
      <div className={styles.track}>
      <input
        id={id}
        className={styles.range}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        aria-valuetext={valueText ?? display}
        aria-describedby={help ? `${id}-help` : undefined}
        style={{ '--pct': `${pct}%` } as CSSProperties}
        onChange={(e) => set(Number(e.target.value))}
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
      />
      {mark && <span className={styles.tick} style={{ left: `calc(${THUMB_REM / 2}rem + (100% - ${THUMB_REM}rem) * ${mark.at})` }} aria-hidden="true" />}
      </div>
      {ticks && ticks.length > 0 && (
        <ol className={styles.ticks} aria-hidden="true">
          {ticks.map((t, i) => (
            <li key={i} style={{ left: `calc(${THUMB_REM / 2}rem + (100% - ${THUMB_REM}rem) * ${t.at})` }}>
              <i />
              <span>{t.label}</span>
            </li>
          ))}
        </ol>
      )}
      {(minLabel || maxLabel) && !(ticks && ticks.length > 0) && (
        <div className={styles.ends} aria-hidden="true">
          <span>{minLabel}</span>
          <span>{maxLabel}</span>
        </div>
      )}
      {mark && (
        <p className={styles.markRow} aria-hidden="true" data-side={mark.at < 0.25 ? 'start' : mark.at > 0.75 ? 'end' : 'mid'}>
          <span className={styles.markArrow} style={{ left: `calc(${THUMB_REM / 2}rem + (100% - ${THUMB_REM}rem) * ${mark.at})` }}>▲</span>
          <span className={styles.markText} style={{ left: `calc(${THUMB_REM / 2}rem + (100% - ${THUMB_REM}rem) * ${mark.at})` }}>{mark.label}</span>
        </p>
      )}
      {help && (
        <p id={`${id}-help`} className={styles.help}>
          {help}
        </p>
      )}
    </div>
  )
}
