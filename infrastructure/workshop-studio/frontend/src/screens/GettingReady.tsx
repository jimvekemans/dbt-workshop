import type { Demo, Mode, ProgressStatus, ServerState } from '../api/types'
import { jobStartedMs } from '../api/adapt'
import { Button } from '../components/Button'
import { Callout } from '../components/Callout'
import { Card } from '../components/Card'
import { PlayIcon } from '../components/icons'
import { StatusMark } from '../components/Status'
import { demoIsPreparing } from '../lib/store'
import { envWords } from '../lib/env'
import { elapsedSeconds, formatDuration, usualText } from '../lib/format'
import { useNow } from '../lib/useNow'
import styles from './GettingReady.module.css'

const MAP = { todo: 'idle', running: 'running', done: 'ok', failed: 'bad' } as const
const MARK = (s: ProgressStatus) => MAP[s] ?? 'idle'

interface Props {
  server: ServerState | null
  demo: Demo | null
  scenarioTitle: string
  /** Presses "Prepare this demo". Nothing is prepared before this is called. */
  onPrepare: () => void
  /** How long getting this demo ready usually takes. */
  usualSeconds?: number | null
}

/** What preparing does, in both modes, in one plain sentence each. The time comes next to it ("Usually about N s"). */
export function prepareSentence(mode: Mode): string {
  return mode === 'replay' ? 'A recording: nothing is changed, this takes a second.' : "Puts the practice system in this demo's starting state."
}

/** The part 4 start: an explicit button, then visible progress while the stack is made ready, or a plain failure. */
export function GettingReady({ server, demo, scenarioTitle, onPrepare, usualSeconds }: Props) {
  const now = useNow(!!server?.job)
  if (!server) {
    return (
      <Card className={styles.card} aria-busy="true">
        <h3 className={styles.title}>Waiting for the studio</h3>
        <p className={styles.text}>As soon as it answers, you can prepare the demo.</p>
      </Card>
    )
  }
  const { baseline, job } = server
  const busy = !!job

  if (demo?.status === 'failed') {
    return (
      <Callout tone="bad" role="alert" title="The demo could not be made ready" actions={<Button variant="primary" disabled={busy} disabledReason="Wait until the current work has finished" onClick={onPrepare}>Try again</Button>}>
        {demo.message || baseline.detail || 'Something stopped the preparation. Nothing was changed for the audience.'}
      </Callout>
    )
  }

  if (demoIsPreparing(demo, server)) {
    const mine = job?.scenario !== null
    const actions = job?.actions ?? []
    const checking = baseline.status === 'preparing' && baseline.progress.length > 0
    const steps = [...(checking ? baseline.progress : []), ...actions]
    const pct = steps.length > 0 ? Math.round((steps.filter((s) => s.status === 'done').length / steps.length) * 100) : null
    const usual = usualText(usualSeconds)
    const replay = server.mode === 'replay'
    const elapsed = job ? formatDuration(elapsedSeconds(jobStartedMs(job), now)) : null
    return (
      <Card className={styles.card} aria-busy="true">
        <p className="kicker">Getting ready</p>
        <h3 className={styles.title}>{mine ? `Getting “${scenarioTitle}” ready` : 'Checking the practice company'}</h3>
        <p className={styles.text}>{job?.label ?? 'Tidying up after the previous demo and setting the scene.'}</p>
        {server.waiting && (
          <p className={styles.text} role="status">
            <StatusMark state="running" /> {server.waiting.text}
          </p>
        )}
        <p className={styles.detail}>
          {elapsed && <b className={styles.elapsed}>{elapsed} so far</b>}
          {elapsed && usual && !replay ? ' · ' : ''}
          {usual && !replay ? usual.charAt(0).toUpperCase() + usual.slice(1) : ''}
          {elapsed || (usual && !replay) ? '.' : ''}
        </p>
        <div className={styles.bar} role="progressbar" aria-label="Getting ready" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined} aria-valuetext={pct === null ? 'In progress' : `${pct} percent`}>
          <span className={styles.fill} style={pct === null ? undefined : { width: `${pct}%`, animation: 'none' }} />
        </div>
        {checking && (
          <ol className={styles.list} aria-label="Checks of the practice company">
            {baseline.progress.map((p, i) => (
              <li key={i} data-state={MARK(p.status)} className={styles.item}>
                <StatusMark state={MARK(p.status)} />
                <span>{envWords(p.label)}</span>
              </li>
            ))}
          </ol>
        )}
        {actions.length > 0 && (
          <ol className={styles.list} aria-label="Setting the scene">
            {actions.map((a, i) => (
              <li key={i} data-state={MARK(a.status)} className={styles.item}>
                <StatusMark state={MARK(a.status)} />
                <span>
                  {a.label}
                  {a.summary && a.status === 'done' ? ` · ${a.summary}` : ''}
                </span>
              </li>
            ))}
          </ol>
        )}
        {baseline.detail && checking && <p className={styles.detail}>{envWords(baseline.detail)}</p>}
      </Card>
    )
  }

  // No demo for this scenario yet (never opened, cleaned up, or another scenario was open): ask first.
  const usual = server.mode === 'live' ? usualText(usualSeconds) : null
  return (
    <Card className={styles.card}>
      <p className="kicker">Before the demo</p>
      <h3 className={styles.title}>Prepare the practice system</h3>
      <p className={styles.text}>
        {prepareSentence(server.mode)}
        {usual ? ` ${usual.charAt(0).toUpperCase() + usual.slice(1)}.` : ''}
      </p>
      {baseline.status === 'failed' && (
        <Callout tone="warn" title="The last check of the practice company failed">
          {baseline.detail}
        </Callout>
      )}
      <div>
        <Button variant="primary" size="lg" icon={<PlayIcon size="0.9em" />} disabled={busy} disabledReason={busy ? `Wait for this to finish: ${job?.label}.` : undefined} onClick={onPrepare}>
          Prepare this demo
        </Button>
      </div>
      {busy && <p className={styles.detail}>{job?.label}. You can start this when it has finished.</p>}
    </Card>
  )
}
