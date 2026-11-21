import type { CatalogIdea, TimelineDay, Visual } from '../api/types'

export type Tone = 'ok' | 'bad' | 'warn' | 'idle' | 'fix'
const TONES: readonly string[] = ['ok', 'bad', 'warn', 'idle', 'fix']

/** A state the UI does not know (a newer backend) is drawn as idle, never as a crash. */
export const visualTone = (s: unknown): Tone => (typeof s === 'string' && TONES.includes(s) ? (s as Tone) : 'idle')

export interface DayGroup {
  from: number
  to: number
  state: Tone
  note: string
  label: string
}

/** Neighbouring days with the same state and the same note become one row ("do tot za"); the marker always splits a group. */
export function groupDays(days: TimelineDay[], markerAfter?: number): DayGroup[] {
  const out: DayGroup[] = []
  days.forEach((d, i) => {
    const state = visualTone(d.state)
    const note = d.note ?? ''
    const last = out[out.length - 1]
    if (last && last.state === state && last.note === note && markerAfter !== i - 1) {
      last.to = i
      last.label = `${days[last.from]?.label ?? ''} tot ${d.label}`
    } else out.push({ from: i, to: i, state, note, label: d.label })
  })
  return out
}

/**
 * The idea of a catalog entry as the home-page preview draws it, or null when there is nothing safe to draw (an old
 * backend sends none; a newer one may send a visual type this UI does not know, or one without its lists).
 */
export function previewIdea(idea: CatalogIdea | null | undefined): CatalogIdea | null {
  const v = idea?.visual as Visual | undefined
  if (!v || typeof v !== 'object') return null
  const sideOk = (x: unknown) => !!x && typeof x === 'object' && typeof (x as { title?: unknown }).title === 'string'
  const ok =
    (v.type === 'flow' && Array.isArray(v.nodes) && v.nodes.length > 0) ||
    (v.type === 'timeline' && Array.isArray(v.days) && v.days.length > 0) ||
    (v.type === 'compare' && sideOk(v.left) && sideOk(v.right))
  return ok ? { headline: typeof idea?.headline === 'string' ? idea.headline : '', visual: v } : null
}
