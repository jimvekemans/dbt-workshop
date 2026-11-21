/** Belgian Dutch everywhere: 3.432 and 92,8, "21 november, 18:06". */
const LOCALE = 'nl-BE'
const DAY_MS = 86_400_000

function parseIso(iso: string): Date {
  const [y, m, d] = iso.slice(0, 10).split('-').map(Number)
  return new Date(Date.UTC(y ?? 1970, (m ?? 1) - 1, d ?? 1))
}
export function toIso(d: Date): string {
  return d.toISOString().slice(0, 10)
}
export function addDays(iso: string, n: number): string {
  return toIso(new Date(parseIso(iso).getTime() + n * DAY_MS))
}
export function diffDays(a: string, b: string): number {
  return Math.round((parseIso(b).getTime() - parseIso(a).getTime()) / DAY_MS)
}

/** "dinsdag 14 oktober 2025" (long) or "di 14 okt" (short) or "dinsdag" (weekday). Always UTC, never shifted. Belgian Dutch. */
export function formatDay(iso: string, style: 'long' | 'short' | 'weekday' | 'noyear' = 'long'): string {
  const d = parseIso(iso)
  const opt: Intl.DateTimeFormatOptions =
    style === 'long'
      ? { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }
      : style === 'noyear'
        ? { weekday: 'long', day: 'numeric', month: 'long' }
        : style === 'short'
          ? { weekday: 'short', day: 'numeric', month: 'short' }
          : { weekday: 'long' }
  return new Intl.DateTimeFormat(LOCALE, { ...opt, timeZone: 'UTC' }).format(d).replace(/[,.]/g, '')
}

export function formatNumber(n: number, maxFraction = 1): string {
  if (!Number.isFinite(n)) return '–'
  return new Intl.NumberFormat(LOCALE, { maximumFractionDigits: maxFraction }).format(n)
}

export function plural(n: number, one: string, many: string): string {
  return `${formatNumber(n)} ${Math.abs(n) === 1 ? one : many}`
}

export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds))
  if (s < 60) return `${s} s`
  const m = Math.floor(s / 60)
  const r = s % 60
  return r === 0 ? `${m} min` : `${m} min ${r} s`
}

export function formatClock(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return new Intl.DateTimeFormat(LOCALE, { hour: '2-digit', minute: '2-digit' }).format(d)
}

/** "21 november, 18:06": the day and the time as two parts, so the comma stays whatever the Intl data of the browser says. */
function dayAndTime(d: Date, seconds: boolean): string {
  const day = new Intl.DateTimeFormat(LOCALE, { day: 'numeric', month: 'long' }).format(d)
  const time = new Intl.DateTimeFormat(LOCALE, { hour: '2-digit', minute: '2-digit', ...(seconds ? { second: '2-digit' as const } : {}) }).format(d)
  return `${day}, ${time}`
}

export function formatWhen(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return dayAndTime(d, false)
}

/** "15 november, 17:16:12": with seconds, so two runs in the same minute do not look like a duplicate. */
export function formatWhenSeconds(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return dayAndTime(d, true)
}

/** Seconds between a start moment in epoch milliseconds and now. */
export function elapsedSeconds(startedMs: number, now: number = Date.now()): number {
  return Number.isFinite(startedMs) ? Math.max(0, (now - startedMs) / 1000) : 0
}

/** "3 meer dan", "5 minder dan", "evenveel als" for the big-number comparison. */
export function compareWords(value: number, other: number, label: string): string {
  const d = value - other
  if (d === 0) return `Evenveel als ${label} (${formatNumber(other)})`
  return `${formatNumber(Math.abs(d))} ${d > 0 ? 'meer' : 'minder'} dan ${label} (${formatNumber(other)})`
}

/** "meestal ongeveer 25 s" (rounded to 5 s), "meestal ongeveer 2 minuten" above 90 s; null when the duration is not known. */
export function usualText(seconds: number | null | undefined): string | null {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds <= 0) return null
  if (seconds > 90) return `meestal ongeveer ${Math.round(seconds / 60)} minuten`
  return `meestal ongeveer ${Math.max(5, Math.round(seconds / 5) * 5)} s`
}

/** "zonet", "7 minuten geleden", otherwise the date and time. */
export function formatAgo(iso: string, now: number = Date.now()): string {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return iso
  const s = (now - t) / 1000
  if (s < 90) return 'zonet'
  if (s < 3600) return `${Math.round(s / 60)} minuten geleden`
  return formatWhen(iso)
}

/** "vandaag", "gisteren", "4 dagen geleden" for a day relative to another day (both ISO dates). */
export function daysAgoText(day: string, today: string): string {
  const n = diffDays(day, today)
  if (n <= 0) return n === 0 ? 'vandaag' : `over ${-n} ${n === -1 ? 'dag' : 'dagen'}`
  return n === 1 ? 'gisteren' : `${n} dagen geleden`
}
