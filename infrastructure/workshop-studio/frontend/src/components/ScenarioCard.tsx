import { useEffect, useId, useRef, useState } from 'react'
import type { CatalogScenario } from '../api/types'
import { plainText } from '../lib/glossary'
import { Card } from './Card'
import { Badge } from './Status'
import { StatusBadge } from './StatusBadge'
import { TechLine } from './TechLine'
import { ScenarioPreview, hasPreview } from './ScenarioPreview'
import { ArrowRightIcon, ClockIcon } from './icons'
import styles from './ScenarioCard.module.css'

/** "Flow · 6 stappen": how a long scenario is marked on its card. */
export function flowLabel(stages: number): string {
  return stages > 0 ? `Flow · ${stages} ${stages === 1 ? 'stap' : 'stappen'}` : 'Flow'
}

/** The pointer must rest on a tile this long before its preview shows, so moving across the page shows nothing. */
export const PREVIEW_DELAY_MS = 450
/** Right after a preview was closed the next one comes quicker: the presenter is going from tile to tile. */
export const PREVIEW_WARM_DELAY_MS = 120
/** A short grace after the pointer leaves, so a small slip does not flicker the preview away. */
export const PREVIEW_LEAVE_MS = 120
const WARM_FOR_MS = 600

let lastClosed = 0

/** Keyboard focus (not a tap or a click) is what shows the preview on focus. */
function focusIsVisible(el: Element): boolean {
  try {
    return el.matches(':focus-visible')
  } catch {
    return true
  }
}

/**
 * One tile of the home page. The whole tile is a link to part 1 (the title is the link; its ::after covers the tile), the
 * link "direct naar de demo" is the one other target. While the pointer rests on the tile (mouse or pen, never touch),
 * or while it has keyboard focus, the idea of the scenario hangs from the tile as a panel.
 */
export function ScenarioCard({ s, number, firstPick }: { s: CatalogScenario; number: number; firstPick?: boolean }) {
  const base = `#/s/${encodeURIComponent(s.id)}`
  const flow = s.kind === 'flow'
  const title = plainText(s.title)
  const promiseId = useId()
  const canPreview = hasPreview(s)
  const [hover, setHover] = useState(false)
  const [focus, setFocus] = useState(false)
  const timer = useRef<number | undefined>(undefined)
  const shown = canPreview && (hover || focus)
  const wasShown = useRef(false)
  const clear = () => window.clearTimeout(timer.current)
  useEffect(() => clear, [])
  useEffect(() => {
    if (wasShown.current && !shown) lastClosed = Date.now()
    wasShown.current = shown
  }, [shown])

  const enter = (e: React.PointerEvent) => {
    if (e.pointerType === 'touch' || !canPreview) return
    clear()
    if (hover) return
    const delay = Date.now() - lastClosed < WARM_FOR_MS ? PREVIEW_WARM_DELAY_MS : PREVIEW_DELAY_MS
    timer.current = window.setTimeout(() => setHover(true), delay)
  }
  const leave = (e: React.PointerEvent) => {
    if (e.pointerType === 'touch') return
    clear()
    if (hover) timer.current = window.setTimeout(() => setHover(false), PREVIEW_LEAVE_MS)
  }
  const onFocus = (e: React.FocusEvent<HTMLElement>) => {
    if (canPreview && focusIsVisible(e.target)) setFocus(true)
  }
  const onBlur = (e: React.FocusEvent<HTMLElement>) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocus(false)
  }
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape' && shown) {
      clear()
      setHover(false)
      setFocus(false)
    }
  }

  return (
    <Card
      as="li"
      className={styles.card}
      data-previewing={shown ? '' : undefined}
      onPointerEnter={enter}
      onPointerLeave={leave}
      onFocus={onFocus}
      onBlur={onBlur}
      onKeyDown={onKeyDown}
    >
      <p className={styles.number} aria-hidden="true">
        {String(number).padStart(2, '0')}
        {firstPick && <span className={styles.pick}>Begin hier</span>}
        {flow && <span className={styles.flow}>{flowLabel(s.steps?.length ?? 0)}</span>}
      </p>
      <h3 className={styles.title}>
        <a className={styles.cover} href={`${base}/problem`} aria-describedby={promiseId}>
          {title}
        </a>
      </h3>
      <div className={styles.body}>
        <p id={promiseId} className={styles.promise}>{plainText(s.promise)}</p>
        <TechLine tech={s.tech} />
      </div>
      <p className={styles.meta}>
        <span className={styles.min}>
          <ClockIcon size="1em" /> Ongeveer {s.minutes} min, inclusief demo
        </span>
        {!s.recorded && <Badge>Enkel live</Badge>}
        {s.status && <StatusBadge status={s.status} />}
      </p>
      <div className={styles.actions}>
        <a className={styles.jump} href={`${base}/demo`} aria-label={`Direct naar de demo: ${title}`}>
          Direct naar de demo
        </a>
        <span className={styles.cue} aria-hidden="true">
          <ArrowRightIcon />
        </span>
      </div>
      {shown && <ScenarioPreview s={s} href={`${base}/problem`} />}
    </Card>
  )
}
