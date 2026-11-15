// Small, pure helpers that turn what the backend really sends into what the views need. Nothing here guesses:
// every function is written against a payload that was recorded from the real studio.
import type { BarsData, EvidenceType, Demo, ErrorData, EvidenceData, EvidenceItem, Job, LiveEvidence, VersionsData } from './types'

/** `job.started` is epoch seconds (a float). */
export function jobStartedMs(job: Pick<Job, 'started'>): number {
  const s = Number(job.started)
  return Number.isFinite(s) ? s * 1000 : Date.now()
}

/** A provider that failed leaves `{error: "plain sentence"}` as its data. */
export function evidenceError(data: LiveEvidence['data']): string | null {
  if (data && typeof data === 'object' && 'error' in data && typeof (data as ErrorData).error === 'string') return (data as ErrorData).error
  return null
}

/** The data of an item when it is real data (not null, not an error marker). */
export function readyData(item: Pick<LiveEvidence, 'data'>): EvidenceData | null {
  const d = item.data
  if (!d || evidenceError(d) !== null) return null
  return d as EvidenceData
}

const SHAPE: Record<EvidenceType, string[]> = {
  table: ['columns', 'rows'], bars: ['series'], number: ['value'], tasks: ['tasks'], pipeline: ['jobs'], mail: ['found'],
  versions: ['environments'], result: ['summary'], freshness: ['sources'], run_history: ['runs'],
}
/**
 * Does this data have the keys the item's type promises? A recording made before a scenario file was edited can hold
 * data for a different item (the backend pairs them by position), and a view must never crash on it.
 */
export function dataMatches(type: EvidenceType, data: unknown): boolean {
  return !!data && typeof data === 'object' && SHAPE[type].every((k) => k in (data as object))
}

export interface BarsModel {
  labels: string[]
  /** Which labels are today (an unfinished day). */
  today: boolean[]
  series: { name: string; state: 'good' | 'bad' | 'neutral'; values: number[] }[]
  /** Sum of all series for one label index. */
  total: (labelIndex: number) => number
}
/** Bars arrive as parallel arrays; short value lists count as zero, and a null value is zero. */
export function barsModel(data: BarsData): BarsModel {
  const rich = Array.isArray(data.bars) && data.bars.length > 0 ? data.bars : null
  const labels = rich ? rich.map((b) => String(b.label)) : (data.labels ?? []).map((l) => String(l))
  const today = rich ? rich.map((b, i) => !!b.today && i === rich.length - 1) : labels.map(() => false)
  // `bars` has the key of each bar; the parallel `series` values follow the old label order (the same order), so they line up by position
  const series = (data.series ?? []).map((s) => ({ name: s.name || 'Total', state: s.state, values: labels.map((_, i) => Number(s.values?.[i] ?? 0) || 0) }))
  return { labels, today, series, total: (i) => series.reduce((a, s) => a + (s.values[i] ?? 0), 0) }
}

export type VersionTags = Record<string, string | null>
export function versionTags(data: VersionsData): VersionTags {
  return Object.fromEntries(data.environments.map((e) => [e.env, e.tag ?? null]))
}

export const versionKey = (scenario: string, step: string, index: number) => `${scenario}/${step}/${index}`

/**
 * The backend only says what runs now. "Changed" needs what ran when the step started, so the first version list we
 * see for an item is kept as its baseline, and dropped again when the item is emptied (the step is run again) or the
 * demo goes away. Returns the same object when nothing changed.
 */
export function nextVersionBaselines(current: Record<string, VersionTags>, demo: Demo | null): Record<string, VersionTags> {
  if (!demo) return Object.keys(current).length === 0 ? current : {}
  const next: Record<string, VersionTags> = {}
  let changed = false
  demo.steps.forEach((step) => {
    step.evidence.forEach((item: EvidenceItem & { data?: LiveEvidence['data'] }, i) => {
      if (item.type !== 'versions') return
      const key = versionKey(demo.scenario, step.id, i)
      const d = readyData(item) as VersionsData | null
      if (!d || !Array.isArray(d.environments)) {
        if (key in current) changed = true
        return
      }
      if (current[key]) next[key] = current[key]
      else {
        next[key] = versionTags(d)
        changed = true
      }
    })
  })
  if (Object.keys(next).length !== Object.keys(current).length) changed = true
  return changed ? next : current
}

/** The version this environment had before the step (undefined when we never saw it). */
export function previousTag(baseline: VersionTags | undefined, env: string): string | null | undefined {
  return baseline && env in baseline ? baseline[env] : undefined
}

/** Every value a number item showed while its step ran, in order (consecutive repeats dropped). Key as `versionKey`. */
export type NumberTrails = Record<string, number[]>

/**
 * The frames of a recorded or live step arrive as successive states; a number card only shows the latest one. This keeps
 * the earlier values, so the card can say "0 → 25,000 → 50,000 → 100,000". An emptied item (the step runs again) or a
 * demo that goes away starts the trail over. Returns the same object when nothing changed.
 */
export function nextNumberTrails(current: NumberTrails, demo: Demo | null): NumberTrails {
  if (!demo) return Object.keys(current).length === 0 ? current : {}
  const next: NumberTrails = {}
  let changed = false
  demo.steps.forEach((step) => {
    step.evidence.forEach((item: EvidenceItem & { data?: LiveEvidence['data'] }, i) => {
      if (item.type !== 'number') return
      const key = versionKey(demo.scenario, step.id, i)
      const d = readyData(item) as { value?: unknown } | null
      const v = d && typeof d.value === 'number' && Number.isFinite(d.value) ? d.value : null
      if (v === null) {
        if (key in current) changed = true
        return
      }
      const before = current[key] ?? []
      if (before[before.length - 1] === v) next[key] = before
      else {
        next[key] = [...before, v]
        changed = true
      }
    })
  })
  if (Object.keys(next).length !== Object.keys(current).length) changed = true
  return changed ? next : current
}

/** At most `max` values for display: the first, the last and an even sample between them. */
export function sampleTrail(values: number[], max = 6): number[] {
  if (values.length <= max) return values
  const out: number[] = []
  for (let k = 0; k < max; k += 1) out.push(values[Math.round((k * (values.length - 1)) / (max - 1))]!)
  return out.filter((v, i) => i === 0 || v !== out[i - 1])
}
