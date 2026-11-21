import { useState } from 'react'
import type { TasksData } from '../api/types'
import { formatClock } from '../lib/format'
import { taskStatus } from '../lib/status'
import { Status } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './TasksView.module.css'

type Task = TasksData['tasks'][number]
/** What a row really is: its state, or "not needed" for a step that was skipped on purpose in a run without a failure. */
type Kind = Task['state'] | 'not_needed'
type Block = { kind: 'row'; t: Task; i: number } | { kind: 'fold'; from: number; to: number; state: Kind }

/** Labels come from the API, which is translated by others: both the English and the Dutch wording are recognised. */
const FAILURE_MAIL = /failure mail|faalmail|foutmail|mail bij (een )?fout/i
const ONE_PART = /\bone part\b|\b(één|een) deel\b/i
const DONE_WORD = /^(done|klaar|gedaan)$/i

/** Is the run over (no row running)? Only then does a row that never started mean "not run". */
function finished(tasks: Task[]): boolean {
  return !tasks.some((t) => t.state === 'running')
}

export function kindOf(t: Task, hasFailure: boolean): Kind {
  if (t.not_needed) return 'not_needed'
  if (t.state === 'skipped' && !hasFailure) return 'not_needed'
  return t.state
}

/**
 * Rows in a run of three or more with the same quiet state fold into one line that says what really happened ("3 more steps
 * not run", "10 more steps skipped"). Never folded: a failed or running row, the failure mail, and everything before the first failure.
 * Steps that were not needed fold anywhere.
 */
export function foldTasks(tasks: Task[]): Block[] {
  const firstFailed = tasks.findIndex((t) => t.state === 'failed')
  const hasFailure = firstFailed >= 0
  const foldable = (t: Task, i: number) => {
    if (FAILURE_MAIL.test(t.label)) return false
    const k = kindOf(t, hasFailure)
    if (k === 'not_needed') return true
    return hasFailure && i > firstFailed && (k === 'skipped' || k === 'done' || k === 'todo')
  }
  const out: Block[] = []
  let i = 0
  while (i < tasks.length) {
    const t = tasks[i]!
    if (foldable(t, i)) {
      const k = kindOf(t, hasFailure)
      let j = i
      while (j < tasks.length && foldable(tasks[j]!, j) && kindOf(tasks[j]!, hasFailure) === k) j += 1
      if (j - i >= 3) {
        out.push({ kind: 'fold', from: i, to: j, state: k })
        i = j
        continue
      }
    }
    out.push({ kind: 'row', t, i })
    i += 1
  }
  return out
}

/** "Deel 1 van 4" for the repeated "Loading one part ..." rows, from the backend's `part`/`parts` or by counting. */
export function partNumbers(tasks: Task[]): Map<number, { part: number; parts: number }> {
  const out = new Map<number, { part: number; parts: number }>()
  const same = new Map<string, number[]>()
  tasks.forEach((t, i) => {
    if (typeof t.part === 'number' && typeof t.parts === 'number') out.set(i, { part: t.part, parts: t.parts })
    else if (ONE_PART.test(t.label) && !(typeof t.count === 'number' && t.count > 1)) same.set(t.label, [...(same.get(t.label) ?? []), i])
  })
  for (const idx of same.values()) if (idx.length >= 2) idx.forEach((i, k) => out.set(i, { part: k + 1, parts: idx.length }))
  return out
}

const FOLD_WORD: Record<Kind, string> = { skipped: 'overgeslagen', todo: 'niet gerund', done: 'klaar', not_needed: 'niet nodig', failed: 'mislukt', running: 'bezig' }

function Row({ t, kind, over, part }: { t: Task; kind: Kind; over: boolean; part?: { part: number; parts: number } }) {
  // a long state sentence ("Skipped because an earlier step failed") is a line of its own; the badge keeps a short word
  const text = t.state_text ?? ''
  // "Done the first time" is a quiet remark, not a status badge
  const quiet = t.state === 'done' && text !== '' && !DONE_WORD.test(text)
  const notNeeded = kind === 'not_needed'
  const notRun = kind === 'todo' && over
  const long = !quiet && !notNeeded && !notRun && text.length > 16
  const s = taskStatus(t.state, long || quiet || notRun ? undefined : text)
  return (
    <li className={styles.item} data-state={notNeeded ? 'idle' : s.state}>
      <span className={styles.label}>
        {part && <span className={styles.part}>Deel {part.part} van {part.parts}</span>}
        {t.label}
        {typeof t.count === 'number' && t.count > 1 && <span className={styles.count}> ×{t.count}</span>}
      </span>
      {long && <span className={styles.detail}>{text}</span>}
      {notNeeded ? (
        <span className={styles.neutral}>Niet nodig</span>
      ) : quiet ? (
        <span className={styles.quiet}>{text}</span>
      ) : notRun ? (
        <Status state="idle">Niet gerund</Status>
      ) : (
        <Status state={s.state}>{s.word}</Status>
      )}
    </li>
  )
}

export function TasksView({ data }: { data: TasksData }) {
  const [open, setOpen] = useState<ReadonlySet<number>>(new Set())
  if (data.tasks.length === 0) return <EmptyNote>{data.empty || 'Er is nog niets gerund.'}</EmptyNote>
  const run = data.run
  const hasFailure = data.tasks.some((t) => t.state === 'failed')
  const over = finished(data.tasks) && run?.state !== 'running'
  const parts = partNumbers(data.tasks)
  return (
    <>
      {run && (
        <p className={styles.run}>
          <Status state={taskStatus(run.state, run.state_text).state} variant="plain">
            {run.state_text}
          </Status>
          {run.started && <span className={styles.when}> · gestart om {formatClock(run.started)}</span>}
        </p>
      )}
      <ol className={styles.list}>
        {foldTasks(data.tasks).map((b) => {
          if (b.kind === 'row') return <Row key={b.i} t={b.t} kind={kindOf(b.t, hasFailure)} over={over && hasFailure} part={parts.get(b.i)} />
          const n = b.to - b.from
          const shown = open.has(b.from)
          return (
            <li key={`f${b.from}`} className={styles.fold} data-open={shown ? '' : undefined}>
              <span>nog {n} stappen {FOLD_WORD[b.state]}</span>
              <button
                type="button"
                className={styles.toggle}
                aria-expanded={shown}
                onClick={() => setOpen((o) => { const x = new Set(o); if (x.has(b.from)) x.delete(b.from); else x.add(b.from); return x })}
              >
                {shown ? 'Verberg' : 'Toon alles'}
              </button>
              {shown && (
                <ol className={styles.inner}>
                  {data.tasks.slice(b.from, b.to).map((t, k) => (
                    <Row key={k} t={t} kind={kindOf(t, hasFailure)} over={over && hasFailure} part={parts.get(b.from + k)} />
                  ))}
                </ol>
              )}
            </li>
          )
        })}
      </ol>
    </>
  )
}
