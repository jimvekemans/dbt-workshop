import { useStudio } from '../lib/studio'
import { envWords } from '../lib/env'
import { formatDay } from '../lib/format'
import type { ServerState, StackService } from '../api/types'
import { StatusMark } from './Status'
import type { StatusState } from './Status'
import { Toggle } from './Toggle'
import { ExternalIcon } from './icons'
import { LeftoversNotice } from './LeftoversNotice'
import styles from './StatusDetails.module.css'

export const OVERVIEW_URL = 'http://localhost:8099'

const MAP: Record<StackService['status'], { state: StatusState; word: string }> = {
  up: { state: 'ok', word: 'Running' },
  down: { state: 'bad', word: 'Not running' },
  starting: { state: 'running', word: 'Starting' },
}
/** What each part of the stack is called for an audience that does not know the tool names, and what it is for. */
const NAMES: Record<string, { label: string; gloss?: string }> = {
  gitlab: { label: 'Code storage (GitLab)' },
  singlestore: { label: 'Company database' },
  'airflow-dev': { label: 'Nightly runs: development', gloss: 'Where changes are tried first.' },
  'airflow-val': { label: 'Nightly runs: validation', gloss: 'Where changes are checked.' },
  'airflow-prd': { label: 'Nightly runs: production', gloss: 'What the business uses.' },
  mailpit: { label: 'Test mailbox' },
}
const BASELINE_WORD = { ready: 'Ready', preparing: 'Being checked', missing: 'Not checked yet', failed: 'Could not be made ready' } as const
const PROGRESS: Record<string, StatusState> = { todo: 'idle', running: 'running', done: 'ok', failed: 'bad' }

/** Everything behind "Details": the services, the practice system, the data switch, what is left over, the overview. */
export function StatusDetails() {
  const { server, catalog, actions } = useStudio()
  if (!server) return null
  return (
    <div className={styles.grid}>
      <section aria-labelledby="svc-h">
        <h2 id="svc-h" className={styles.h}>
          Everything the demos need
        </h2>
        <p className={styles.sub}>Green means ready.</p>
        <ul className={styles.list}>
          {server.stack.map((s) => {
            const m = MAP[s.status]
            const n = NAMES[s.id]
            return (
              <li key={s.id} className={styles.item} data-state={m.state} title={n?.gloss}>
                <StatusMark state={m.state} />
                <span className={styles.name}>{n?.label ?? s.label}</span>
                <span className={styles.word}>{m.word}</span>
                {s.detail && <span className={styles.detail}>{s.detail}</span>}
              </li>
            )
          })}
        </ul>
        <Baseline server={server} />
      </section>
      <section className={styles.side} aria-label="More">
        <Toggle
          label="Practice data keeps coming in"
          checked={server.feeder.running}
          onChange={(v) => void actions.setFeeder(v)}
          disabled={!!server.job}
          help={
            server.job
              ? 'You can change this when the current work has finished.'
              : server.feeder.running
                ? `Made-up orders and deliveries arrive every minute. Switch off to freeze the numbers.${server.feeder.simulated_day ? ` The practice company is on ${formatDay(server.feeder.simulated_day, 'noyear')}.` : ''}`
                : 'Frozen. Numbers stay as they are.'
          }
        />
        <LeftoversNotice />
        <a href={catalog?.overview_url || OVERVIEW_URL} target="_blank" rel="noreferrer" className={styles.link} title="The presenter's own dashboard">
          Open the system overview (new tab) <ExternalIcon size="0.9em" />
        </a>
      </section>
    </div>
  )
}

/** The practice company: data, a numbered version and built tables in each environment. */
function Baseline({ server }: { server: ServerState }) {
  const { baseline } = server
  const state: StatusState = baseline.status === 'ready' ? 'ok' : baseline.status === 'failed' ? 'bad' : baseline.status === 'preparing' ? 'running' : 'idle'
  const show = baseline.status !== 'ready' && baseline.progress.length > 0
  return (
    <div className={styles.baseline}>
      <p className={styles.item} data-state={state}>
        <StatusMark state={state} />
        <span className={styles.name}>The practice system</span>
        <span className={styles.word}>{BASELINE_WORD[baseline.status]}</span>
        {baseline.status !== 'ready' && baseline.detail && <span className={styles.detail}>{envWords(baseline.detail)}</span>}
      </p>
      {show && (
        <ol className={styles.progress} aria-label="Checks of the practice system">
          {baseline.progress.map((p, i) => (
            <li key={i} data-state={PROGRESS[p.status] ?? 'idle'}>
              <StatusMark state={PROGRESS[p.status] ?? 'idle'} />
              <span>{envWords(p.label)}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}
