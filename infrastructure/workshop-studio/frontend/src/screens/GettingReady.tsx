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
  return mode === 'replay' ? 'Een opname: er wordt niets gewijzigd, dit duurt een seconde.' : 'Zet het oefensysteem in de beginstand van deze demo.'
}

/** The part 4 start: an explicit button, then visible progress while the stack is made ready, or a plain failure. */
export function GettingReady({ server, demo, scenarioTitle, onPrepare, usualSeconds }: Props) {
  const now = useNow(!!server?.job)
  if (!server) {
    return (
      <Card className={styles.card} aria-busy="true">
        <h3 className={styles.title}>Wachten op de Studio</h3>
        <p className={styles.text}>Zodra ze antwoordt, kan je de demo klaarzetten.</p>
      </Card>
    )
  }
  const { baseline, job } = server
  const busy = !!job

  if (demo?.status === 'failed') {
    return (
      <Callout tone="bad" role="alert" title="De demo kon niet klaargezet worden" actions={<Button variant="primary" disabled={busy} disabledReason="Wacht tot het huidige werk klaar is" onClick={onPrepare}>Opnieuw proberen</Button>}>
        {demo.message || baseline.detail || 'Er stopte iets bij het klaarzetten. Voor het publiek is er niets gewijzigd.'}
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
        <p className="kicker">Klaarzetten</p>
        <h3 className={styles.title}>{mine ? `“${scenarioTitle}” wordt klaargezet` : 'Het oefensysteem wordt nagekeken'}</h3>
        <p className={styles.text}>{job?.label ?? 'Opkuisen na de vorige demo en het decor klaarzetten.'}</p>
        {server.waiting && (
          <p className={styles.text} role="status">
            <StatusMark state="running" /> {server.waiting.text}
          </p>
        )}
        <p className={styles.detail}>
          {elapsed && <b className={styles.elapsed}>{elapsed} tot nu toe</b>}
          {elapsed && usual && !replay ? ' · ' : ''}
          {usual && !replay ? usual.charAt(0).toUpperCase() + usual.slice(1) : ''}
          {elapsed || (usual && !replay) ? '.' : ''}
        </p>
        <div className={styles.bar} role="progressbar" aria-label="Klaarzetten" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct ?? undefined} aria-valuetext={pct === null ? 'Bezig' : `${pct} procent`}>
          <span className={styles.fill} style={pct === null ? undefined : { width: `${pct}%`, animation: 'none' }} />
        </div>
        {checking && (
          <ol className={styles.list} aria-label="Checks van het oefensysteem">
            {baseline.progress.map((p, i) => (
              <li key={i} data-state={MARK(p.status)} className={styles.item}>
                <StatusMark state={MARK(p.status)} />
                <span>{envWords(p.label)}</span>
              </li>
            ))}
          </ol>
        )}
        {actions.length > 0 && (
          <ol className={styles.list} aria-label="Decor klaarzetten">
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
      <p className="kicker">Voor de demo</p>
      <h3 className={styles.title}>Zet het oefensysteem klaar</h3>
      <p className={styles.text}>
        {prepareSentence(server.mode)}
        {usual ? ` ${usual.charAt(0).toUpperCase() + usual.slice(1)}.` : ''}
      </p>
      {baseline.status === 'failed' && (
        <Callout tone="warn" title="De laatste check van het oefensysteem is mislukt">
          {baseline.detail}
        </Callout>
      )}
      <div>
        <Button variant="primary" size="lg" icon={<PlayIcon size="0.9em" />} disabled={busy} disabledReason={busy ? `Wacht tot dit klaar is: ${job?.label}.` : undefined} onClick={onPrepare}>
          Demo klaarzetten
        </Button>
      </div>
      {busy && <p className={styles.detail}>{job?.label}. Je kan dit starten als het klaar is.</p>}
    </Card>
  )
}
