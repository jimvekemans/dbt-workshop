import { useLayoutEffect, useRef, useState } from 'react'
import type { TimelineVisual as Timeline } from '../api/types'
import { groupDays, visualTone } from '../lib/visual'
import { Status } from './Status'
import { RichText } from './Term'
import { VisualCaption } from './VisualCaption'
import styles from './TimelineVisual.module.css'

const WORD = { bad: 'Probleem', warn: 'Opgelet', idle: 'Wacht', fix: 'De oplossing' } as const

function Word({ state }: { state: ReturnType<typeof visualTone> }) {
  if (state === 'ok') return null
  return <Status state={state === 'fix' ? 'idle' : state} variant="plain" className={styles.state}>{WORD[state]}</Status>
}

function Marker({ label }: { label: string }) {
  return (
    <div className={styles.marker} role="note">
      <span className={styles.markerLabel}><RichText text={label} /></span>
    </div>
  )
}

/** Every day column needs this much room (in rem) for its label and note to be read in full; otherwise the days are stacked. */
const MIN_COLUMN_REM = 7.5

/** Is there room for `count` columns side by side? Measured, not guessed, so a label is never cut off. */
function useRoomFor(count: number) {
  const ref = useRef<HTMLElement>(null)
  const [wide, setWide] = useState(false)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const measure = () => {
      const rem = parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
      setWide(el.clientWidth >= count * MIN_COLUMN_REM * rem)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    return () => ro.disconnect()
  }, [count])
  return [ref, wide] as const
}

/** Day columns side by side with the marker as a vertical dashed line; on a phone equal neighbouring days share one row.
 *  `compact`: the small version of the home-page preview (the grouped days as chips, no columns, no notes). */
export function TimelineVisual({ visual, compact }: { visual: Timeline; compact?: boolean }) {
  const after = visual.marker?.after
  const groups = groupDays(visual.days, after)
  const [ref, wide] = useRoomFor(visual.days.length)
  return (
    <figure ref={ref} className={styles.figure} data-wide={wide && !compact ? '' : undefined} data-compact={compact ? '' : undefined}>
      {!compact && <ol className={styles.cols}>
        {visual.days.map((d, i) => {
          const st = visualTone(d.state)
          return (
            <li key={i} className={styles.slot}>
              <div className={styles.day} data-state={st}>
                <span className={styles.label}><RichText text={d.label} /></span>
                <Word state={st} />
                {d.note && (
                  <span className={styles.note}>
                    <RichText text={d.note} />
                  </span>
                )}
              </div>
              {visual.marker && after === i && <Marker label={visual.marker.label} />}
            </li>
          )
        })}
      </ol>}
      <ol className={styles.rows}>
        {groups.map((g) => (
          <li key={g.from} className={styles.slot}>
            <div className={styles.day} data-state={g.state}>
              <span className={styles.label}><RichText text={g.label} /></span>
              <Word state={g.state} />
              {g.note && !compact && (
                <span className={styles.note}>
                  <RichText text={g.note} />
                </span>
              )}
            </div>
            {visual.marker && after === g.to && <Marker label={visual.marker.label} />}
          </li>
        ))}
      </ol>
      <VisualCaption text={visual.caption} />
    </figure>
  )
}
