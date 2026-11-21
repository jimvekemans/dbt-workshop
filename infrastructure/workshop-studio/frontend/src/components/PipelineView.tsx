import type { PipelineData, PipelineFailure } from '../api/types'
import { taskStatus } from '../lib/status'
import { formatNumber } from '../lib/format'
import { Status } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './PipelineView.module.css'

/** Which job a failure belongs to: the failed job whose name fits its kind, otherwise the first failed one. */
function ownerOf(data: PipelineData, f: PipelineFailure): number {
  const failed = data.jobs.map((j, i) => (j.state === 'failed' ? i : -1)).filter((i) => i >= 0)
  if (failed.length === 0) return -1
  const re = f.kind === 'check' ? /check|test|valid|quality|kwaliteit/i : /build|run|table|tabel|model|load/i
  return failed.find((i) => re.test(data.jobs[i]?.label ?? '')) ?? failed[0]!
}

/** A row of stages that follow one another, each with its own status. */
export function PipelineView({ data }: { data: PipelineData }) {
  if (data.jobs.length === 0) return <EmptyNote>{data.state_text ? `${data.state_text}.` : 'Er is nog niets gestart.'}</EmptyNote>
  const overall = taskStatus(data.state, data.state_text)
  const failures = Array.isArray(data.failures) ? data.failures.filter((f) => f && f.text) : []
  const under = data.jobs.map((_, i) => failures.filter((f) => ownerOf(data, f) === i))
  const orphans = failures.filter((f) => ownerOf(data, f) < 0)
  const line = (f: PipelineFailure, i: number) => (
    <p key={i} className={styles.failure}>
      <b>Wat mislukte:</b> {f.text}
      {typeof f.rows === 'number' && <span className={styles.rows}> · {formatNumber(f.rows)} {f.rows === 1 ? 'rij' : 'rijen'}</span>}
    </p>
  )
  const re = data.rehearsed
  const total = re && typeof re.tables_total === 'number' && re.tables_total > 0 ? re.tables_total : null
  const rehearsed =
    re && typeof re.tables === 'number'
      ? `${formatNumber(re.tables)}${total !== null ? ` van ${formatNumber(total)}` : ''} ${(total ?? re.tables) === 1 ? 'tabel' : 'tabellen'} in de testrun`
      : null
  const quiet = re
    ? [
        Array.isArray(re.names) && re.names.length > 0 ? re.names.join(', ') : null,
        typeof re.checks === 'number' ? `${formatNumber(re.checks)} ${re.checks === 1 ? 'datacheck' : 'datachecks'}` : null,
        // whole seconds, at least 1: the same number as in the step's sentence ("in 1 seconde")
        typeof re.seconds === 'number' ? ((n) => `${n} ${n === 1 ? 'seconde' : 'seconden'}`)(Math.max(1, Math.round(re.seconds))) : null,
      ].filter(Boolean)
    : []
  return (
    <>
      {rehearsed && re && (
        <div className={styles.rehearsedBox}>
          <p className={styles.rehearsed}>
            <b>{rehearsed}</b>
          </p>
          {total !== null && (
            <div className={styles.prop} role="img" aria-label={`${re.tables} van ${total} tabellen`}>
              <span style={{ width: `${Math.max(1.5, Math.min(100, (re.tables / total) * 100))}%` }} />
            </div>
          )}
          {quiet.length > 0 && <p className={styles.rehearsedQuiet}>{quiet.join(' · ')}</p>}
        </div>
      )}
      <p className={styles.overall}>
        <Status state={overall.state} variant="plain">
          {overall.word}
        </Status>
      </p>
      <ol className={styles.row} aria-label="Stappen, op volgorde">
        {data.jobs.map((s, i) => {
          const st = taskStatus(s.state, s.state_text)
          return (
            <li key={i} className={styles.stage} data-state={st.state} data-wide={(under[i]?.length ?? 0) > 0 ? '' : undefined}>
              <span className={styles.index} aria-hidden="true">
                {i + 1}
              </span>
              <span className={styles.name}>{s.label}</span>
              <Status state={st.state} variant="plain">
                {st.word}
              </Status>
              {(under[i] ?? []).map(line)}
            </li>
          )
        })}
      </ol>
      {orphans.map(line)}
    </>
  )
}
