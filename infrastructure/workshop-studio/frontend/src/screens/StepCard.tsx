import { useEffect, useRef } from 'react'
import type { DemoStep, InputValue, InputValues, LiveEvidence, StepDef, TableData, PipelineData, ResultData } from '../api/types'
import { Button } from '../components/Button'
import { Callout } from '../components/Callout'
import { Disclosure } from '../components/Disclosure'
import { EvidenceView } from '../components/EvidenceView'
import { InputField } from '../components/InputField'
import { RichText } from '../components/Term'
import { Status, StatusMark } from '../components/Status'
import { LockIcon, PlayIcon, RefreshIcon } from '../components/icons'
import { readyData, versionKey } from '../api/adapt'
import { useStudio } from '../lib/studio'
import { valueWords } from '../lib/inputs'
import { usualText } from '../lib/format'
import { cx } from '../lib/cx'
import styles from './StepCard.module.css'

interface Props {
  scenarioId: string
  index: number
  total: number
  def: StepDef
  state: DemoStep | null
  blocked: string | null
  values: InputValues
  onValue: (id: string, v: InputValue) => void
  onRun: () => void
  /** The recording is played instead of the real thing: inputs are fixed, the button says so. */
  replay?: boolean
  /** The final evidence of another step (for `before_of`), or null when it has none yet. */
  beforeFor?: (stepId: string, type: string) => LiveEvidence | null
}

const ACTION_STATE = { todo: 'idle', running: 'running', done: 'ok', failed: 'bad' } as const
const wide = (t: string) => ['bars', 'mail', 'run_history', 'table', 'pipeline', 'tasks', 'versions'].includes(t)
/** Small pictures stand side by side as "Before | After"; big ones (charts, tables, lists) are stacked, each complete. */
const PAIRED = new Set(['versions', 'number', 'result', 'pipeline', 'freshness'])

/** The label of a step's button: the same text is used in "press ... to start again". */
export function buttonLabel(def: StepDef, status: string, replay: boolean): string {
  if (status === 'done') return replay ? 'Play the recording again' : 'Run again'
  if (status === 'failed') return 'Try this step again'
  return replay ? `Play the recording: ${def.button}` : def.button
}

export function StepCard({ scenarioId, index, total, def, state, blocked, values, onValue, onRun, replay = false, beforeFor }: Props) {
  const ref = useRef<HTMLElement>(null)
  const heading = useRef<HTMLHeadingElement>(null)
  const outcome = useRef<HTMLDivElement>(null)
  const failure = useRef<HTMLDivElement>(null)
  const { versionBaselines, numberTrails } = useStudio()
  const status = state?.status ?? 'todo'
  const running = status === 'running'
  const done = status === 'done'
  const failed = status === 'failed'
  const stopped = status === 'stopped'
  const notQuite = done && state?.outcome_kind === 'not_quite'
  const evidence = state && state.evidence.length > 0 ? state.evidence : def.show
  const failedAction = state?.actions.find((a) => a.status === 'failed')
  const withDetails = (state?.actions ?? []).filter((a) => a.details)
  // the technical names behind the plain words of a table's cells belong here, never in the cell itself
  const rowDetails = evidence.flatMap((e) => {
    const d = readyData(e as LiveEvidence)
    const name = e.title ?? ''
    if (!d) return []
    if (e.type === 'table') {
      const t = d as TableData
      if (!Array.isArray(t.rows)) return []
      const first = t.columns?.[0]?.key
      return t.rows.filter((r) => r.details).map((r) => ({ label: `${name || 'Table'}${first && r.cells[first] != null ? `: ${String(r.cells[first])}` : ''}`, details: r.details as string }))
    }
    if (e.type === 'pipeline') {
      const p = d as PipelineData
      return (Array.isArray(p.failures) ? p.failures : []).filter((f) => f && f.details).map((f) => ({ label: `${name || 'Checks'}: ${f.text}`, details: f.details as string }))
    }
    if (e.type === 'result') {
      const r = d as ResultData
      return typeof r.details === 'string' && r.details ? [{ label: name || 'What happened', details: r.details }] : []
    }
    return []
  })
  const usual = usualText(def.usual_seconds)
  const label = buttonLabel(def, status, replay)
  const locked = blocked !== null && !running
  const outcomeText = state?.outcome || def.outcome
  const waitsForStep = locked && /^Finish /.test(blocked ?? '')
  const waitingText = waitsForStep ? `Appears after step ${index}.` : 'Appears after you press the button above.'

  // focus follows the work: the heading when a step starts, the result when it ends; the card scrolls into view
  const was = useRef(status)
  useEffect(() => {
    const before = was.current
    was.current = status
    if (before === status) return
    const to = status === 'running' ? heading : before === 'running' ? (outcome.current ? outcome : failure.current ? failure : heading) : null
    if (!to) return
    to.current?.focus({ preventScroll: true })
    const reduce = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
    ref.current?.scrollIntoView?.({ behavior: reduce ? 'auto' : 'smooth', block: 'start' })
  }, [status])

  return (
    <article ref={ref} className={cx(styles.card, running && styles.running, done && styles.done, failed && styles.failed, locked && styles.locked)} aria-labelledby={`step-${def.id}`} data-status={status}>
      <header className={styles.head}>
        <span className={styles.num} aria-hidden="true">
          {done ? '✓' : failed ? '!' : index + 1}
        </span>
        <div className={styles.titles}>
          <p className="kicker">
            Step {index + 1} of {total}
          </p>
          <h3 id={`step-${def.id}`} ref={heading} tabIndex={-1} className={styles.title}>
            <RichText text={def.title} />
          </h3>
        </div>
        <span className={styles.stat}>
          {status === 'todo' && <Status state="idle">{locked ? 'Not yet' : 'Ready'}</Status>}
          {running && <Status state="running">Happening now</Status>}
          {running && usual && <span className={styles.usual}>{usual}</span>}
          {done && <Status state="ok">Done</Status>}
          {failed && <Status state="bad">Did not work</Status>}
          {stopped && <Status state="idle">Stopped</Status>}
        </span>
      </header>

      {def.say && (
        <div className={styles.say}>
          <p className={styles.label}>What happens</p>
          <p>
            <RichText text={def.say.replace(/^\s*say\s*:\s*/i, '')} />
          </p>
        </div>
      )}

      {def.inputs.length > 0 && (
        <div className={styles.inputs}>
          {def.inputs.map((inp) => (
            <InputField key={inp.id} input={inp} value={values[inp.id]} onChange={(v) => onValue(inp.id, v)} disabled={running || locked} replay={replay} />
          ))}
          {replay && (
            <p className={styles.fixed}>
              Fixed in the recording: {def.inputs.map((inp) => valueWords(inp, inp.default)).join('; ')}. Switch to Live to choose yourself.
            </p>
          )}
        </div>
      )}

      <div className={styles.action}>
        <Button
          variant={done ? 'secondary' : 'primary'}
          size="lg"
          loading={running}
          disabled={locked}
          disabledReason={blocked ?? undefined}
          icon={locked ? <LockIcon size="0.9em" /> : done || failed ? <RefreshIcon /> : <PlayIcon size="0.9em" />}
          onClick={onRun}
          aria-describedby={locked ? `why-${def.id}` : undefined}
        >
          {running ? 'Working…' : label}
        </Button>
        {locked && (
          <p id={`why-${def.id}`} className={styles.why}>
            {blocked}
          </p>
        )}
        {!locked && !running && usual && <p className={styles.why}>{usual.charAt(0).toUpperCase() + usual.slice(1)}</p>}
        {running && replay && <p className={styles.why}>A recording cannot be stopped; it takes a few seconds.</p>}
        {done && !running && <p className={styles.why}>{replay ? 'Plays the recording again; the results below are replaced.' : 'Runs this step again; the results below are replaced.'}</p>}
      </div>

      {stopped && (
        <p className={styles.stopped}>
          Stopped. Nothing was lost; press “{buttonLabel(def, 'todo', replay)}” to start again.
        </p>
      )}
      {status === 'todo' && state?.summary && <p className={styles.why}>{state.summary}</p>}

      {(running || (state?.actions.length ?? 0) > 0) && (
        <ol className={styles.progress} aria-label="What is happening" aria-live="polite">
          {(state?.actions ?? []).map((a, i) => (
            <li key={i} className={styles.act} data-state={ACTION_STATE[a.status]}>
              <StatusMark state={ACTION_STATE[a.status]} />
              <span>
                <b>{a.label}</b>
                {a.summary && a.summary.replace(/\.$/, '') !== a.label.replace(/\.$/, '') && <span className={styles.sum}> {a.summary}</span>}
              </span>
            </li>
          ))}
        </ol>
      )}

      {failed && (
        <Callout tone="bad" role="alert" title="This step did not work" boxRef={failure}>
          {state?.summary || failedAction?.summary || 'The step did not finish.'} Nothing is broken: press “{label}”.
        </Callout>
      )}

      {failed ? (
        <p className={styles.nothing}>Nothing to show yet: the step stopped before it could finish.</p>
      ) : (
        evidence.length > 0 && (
          <div className={styles.evidence}>
            {evidence.map((ev, i) => {
              const before = ev.before_of && beforeFor ? beforeFor(ev.before_of, ev.type) : null
              const key = versionKey(scenarioId, def.id, i)
              const after = <EvidenceView item={ev} stepStatus={status} previous={versionBaselines[key]} trail={numberTrails[key]} waitingText={waitingText} />
              if (!before) return <div key={i} className={wide(ev.type) ? styles.wide : undefined}>{after}</div>
              return (
                <div key={i} className={cx(styles.wide, styles.pair)} data-layout={PAIRED.has(ev.type) ? 'side' : 'stack'}>
                  <div className={styles.beforeSide} role="group" aria-label="Before">
                    <p className={styles.sideLabel}>Before</p>
                    <div className={styles.old} inert>
                      <EvidenceView item={before} stepStatus="done" />
                    </div>
                  </div>
                  <div className={styles.afterSide} role="group" aria-label="After">
                    <p className={cx(styles.sideLabel, styles.afterLabel)}>After</p>
                    {after}
                  </div>
                </div>
              )
            })}
          </div>
        )
      )}

      {done && outcomeText && (
        <Callout tone={notQuite ? 'warn' : 'good'} role="status" title={notQuite ? 'Not quite' : 'What just happened'} boxRef={outcome}>
          <RichText text={outcomeText} />
        </Callout>
      )}

      {(withDetails.length > 0 || rowDetails.length > 0) && (
        <Disclosure summary="Technical details (for engineers)">
          <div className={styles.tech}>
            {withDetails.map((a, i) => (
              <div key={i}>
                <p className={styles.techLabel}>{a.label}</p>
                <pre className={styles.pre} tabIndex={0}>{a.details}</pre>
              </div>
            ))}
            {rowDetails.map((r, i) => (
              <div key={`r${i}`}>
                <p className={styles.techLabel}>{r.label}</p>
                <pre className={styles.pre} tabIndex={0}>{r.details}</pre>
              </div>
            ))}
          </div>
        </Disclosure>
      )}
    </article>
  )
}

/** The data a step ended with, for the "Before" of a later step: the last item of this type that has data. */
export function finalEvidence(state: DemoStep | null, type: string): LiveEvidence | null {
  const items = (state?.evidence ?? []).filter((e) => e.type === type && readyData(e))
  return items[items.length - 1] ?? null
}
