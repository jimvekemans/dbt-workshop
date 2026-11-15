import type { RunHistoryData } from '../api/types'
import { formatWhenSeconds } from '../lib/format'
import { Status } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './RunHistoryView.module.css'

type Run = RunHistoryData['runs'][number]
type Tone = 'bad' | 'warn' | undefined

const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/** What went wrong in a run: a failed table or check, or the backend's own verdict. */
export function runFailed(r: Run): boolean {
  return r.state === 'bad' || (r.tables_failed ?? 0) > 0 || (r.checks_failed ?? 0) > 0
}

/** The headline of a run: "No problems" only with no failure and no warning, "1 warning" with warnings only, otherwise "Problems". */
export function runVerdict(r: Run): { state: 'ok' | 'warn' | 'bad'; word: string } {
  if (runFailed(r)) return { state: 'bad', word: 'Problems' }
  const w = r.checks_warned ?? 0
  if (r.state === 'warn' && w === 0) return { state: 'warn', word: '1 warning' }
  if (w > 0) return { state: 'warn', word: plural(w, 'warning', 'warnings') }
  return { state: 'ok', word: 'No problems' }
}

/** Small labelled numbers (each one says its unit, the same words as the sentence); only the ones the backend sent. */
export function runChips(r: Run): { text: string; tone: Tone }[] {
  const out: { text: string; tone: Tone }[] = []
  // "25 of 28 tables built, 3 left out on purpose": the project's tables, not only the ones in this run; a retry says what it built
  const projectTotal = num(r.tables_total) ? r.tables_total + (num(r.tables_left_out) ? r.tables_left_out : 0) : null
  if (num(r.tables_built) && projectTotal !== null && !r.retry) out.push({ text: `${r.tables_built} of ${projectTotal} tables built`, tone: undefined })
  else if (num(r.tables_built)) out.push({ text: plural(r.tables_built, 'table built', 'tables built'), tone: undefined })
  if (num(r.tables_left_out) && r.tables_left_out > 0) out.push({ text: `${r.tables_left_out} left out on purpose`, tone: undefined })
  const notRun = num(r.tables_not_run) ? r.tables_not_run : num(r.not_run) ? r.not_run : 0
  if (notRun > 0) out.push({ text: num(r.tables_not_run) ? plural(notRun, 'table not run', 'tables not run') : `${notRun} not run`, tone: undefined })
  if (num(r.tables_failed) && r.tables_failed > 0) out.push({ text: plural(r.tables_failed, 'table failed', 'tables failed'), tone: 'bad' })
  if (num(r.checks_failed) && r.checks_failed > 0) out.push({ text: plural(r.checks_failed, 'check failed', 'checks failed'), tone: 'bad' })
  const passed = num(r.checks_passed) ? r.checks_passed : num(r.checks_total) && !(r.checks_failed && r.checks_failed > 0) ? r.checks_total : null
  if (passed !== null && passed > 0) out.push({ text: `${plural(passed, 'check', 'checks')} passed`, tone: undefined })
  if (num(r.checks_warned) && r.checks_warned > 0) out.push({ text: plural(r.checks_warned, 'warning', 'warnings'), tone: 'warn' })
  if (num(r.checks_not_run) && r.checks_not_run > 0) out.push({ text: plural(r.checks_not_run, 'check not run', 'checks not run'), tone: undefined })
  return out
}

function Row({ r, first }: { r: Run; first?: boolean }) {
  const verdict = runVerdict(r)
  const cs = runChips(r)
  return (
    <li className={styles.item} data-state={verdict.state} data-first={first ? '' : undefined}>
      <span className={styles.when}>
        {first && <b className={styles.now}>{r.current ? 'This run' : 'Latest'}</b>}
        {r.retry && <b className={styles.retry}>Retry</b>}
        <span className={styles.time}>{formatWhenSeconds(r.started)}</span>
      </span>
      <div className={styles.label}>
        <span>{r.summary}</span>
        {cs.length > 0 && (
          <ul className={styles.chips} aria-label="In numbers">
            {cs.map((c) => (
              <li key={c.text} className={styles.chip} data-tone={c.tone}>
                {c.text}
              </li>
            ))}
          </ul>
        )}
      </div>
      <Status state={verdict.state}>{verdict.word}</Status>
    </li>
  )
}

/** Newest night first and marked "This run"; the older ones are labelled as not part of this demo. */
export function RunHistoryView({ data, limit }: { data: RunHistoryData; limit?: number }) {
  if (data.runs.length === 0) return <EmptyNote>{data.empty || 'Nothing has run yet.'}</EmptyNote>
  const runs = [...data.runs].sort((a, b) => b.started.localeCompare(a.started)).slice(0, limit ?? 8)
  const hi = Math.max(0, runs.findIndex((r) => r.current))
  const newest = runs[hi]
  const older = runs.filter((_, i) => i !== hi)
  return (
    <>
      <ol className={styles.list}>{newest && <Row r={newest} first />}</ol>
      {older.length > 0 && (
        <>
          <p className={styles.earlier}>Earlier nights (not part of this demo)</p>
          <ol className={styles.list}>
            {older.map((r, i) => (
              <Row key={`${r.started}-${i}`} r={r} />
            ))}
          </ol>
        </>
      )}
    </>
  )
}
