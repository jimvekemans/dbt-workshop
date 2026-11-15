import type { TimelineDay } from '../api/types'

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

/** Neighbouring days with the same state and the same note become one row ("Thu to Sat"); the marker always splits a group. */
export function groupDays(days: TimelineDay[], markerAfter?: number): DayGroup[] {
  const out: DayGroup[] = []
  days.forEach((d, i) => {
    const state = visualTone(d.state)
    const note = d.note ?? ''
    const last = out[out.length - 1]
    if (last && last.state === state && last.note === note && markerAfter !== i - 1) {
      last.to = i
      last.label = `${days[last.from]?.label ?? ''} to ${d.label}`
    } else out.push({ from: i, to: i, state, note, label: d.label })
  })
  return out
}
