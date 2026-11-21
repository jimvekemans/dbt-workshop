import { addDays, daysAgoText, diffDays, formatDay } from '../lib/format'
import { Slider } from './Slider'
import { IconButton } from './IconButton'
import { ChevronLeftIcon, ChevronRightIcon } from './icons'
import styles from './DayPicker.module.css'

interface Props {
  label: string
  value: string
  min: string
  max: string
  onChange: (iso: string) => void
  help?: string
  disabled?: boolean
  /** A day the scenario names (for example the real change day), shown as a tick on the track. */
  mark?: { day: string; label: string }
}

/** A slider over the allowed days: the chosen date spelled out with its weekday and how long ago it was; the last day is today. */
export function DayPicker({ label, value, min, max, onChange, help, disabled, mark }: Props) {
  const total = Math.max(0, diffDays(min, max))
  const index = Math.min(total, Math.max(0, diffDays(min, value)))
  const day = addDays(min, index)
  const readable = formatDay(day, 'long')
  const ago = daysAgoText(day, max)
  const go = (i: number) => onChange(addDays(min, Math.min(total, Math.max(0, i))))
  // a small tick with the day number for every day (every second one on a long track); today's end says "today"
  const stride = total > 16 ? 2 : 1
  const ticks = total > 0 ? Array.from({ length: total + 1 }, (_, i) => i).filter((i) => i % stride === 0 || i === total).map((i) => ({ at: i / total, label: i === total ? 'vandaag' : String(Number(addDays(min, i).slice(8, 10))) })) : undefined
  const markAt = mark && total > 0 ? diffDays(min, mark.day) / total : null
  return (
    <div className={styles.picker}>
      <div className={styles.date} aria-live="polite">
        <span className={styles.weekday}>{formatDay(day, 'weekday')}</span>
        <span className={styles.full}>{readable.replace(/^\w+ /, '')}</span>
        <span className={styles.ago}>{ago}</span>
      </div>
      <div className={styles.row}>
        <IconButton className={styles.edge} label="Een dag vroeger" disabled={disabled || index <= 0} disabledReason="Dit is de vroegste dag die je kan kiezen" onClick={() => go(index - 1)}>
          <ChevronLeftIcon />
        </IconButton>
        <div className={styles.slider}>
          <Slider
            label={label}
            value={index}
            min={0}
            max={total}
            onChange={go}
            valueText={`${readable}, ${ago}`}
            disabled={disabled}
            ticks={ticks}
            help={help}
            mark={mark && markAt !== null && markAt >= 0 && markAt <= 1 ? { at: markAt, label: mark.label } : undefined}
          />
        </div>
        <IconButton className={styles.edge} label="Een dag later" disabled={disabled || index >= total} disabledReason="Dit is de laatste dag die je kan kiezen" onClick={() => go(index + 1)}>
          <ChevronRightIcon />
        </IconButton>
      </div>
    </div>
  )
}
