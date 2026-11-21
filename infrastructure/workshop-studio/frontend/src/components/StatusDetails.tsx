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
  up: { state: 'ok', word: 'Draait' },
  down: { state: 'bad', word: 'Draait niet' },
  starting: { state: 'running', word: 'Start op' },
}
/** What each part of the stack is called for an audience that does not know the tool names, and what it is for. */
const NAMES: Record<string, { label: string; gloss?: string }> = {
  gitlab: { label: 'Codeserver (GitLab)' },
  singlestore: { label: 'Bedrijfsdatabase' },
  'airflow-dev': { label: 'Nachtelijke load: development', gloss: 'Waar wijzigingen eerst uitgeprobeerd worden.' },
  'airflow-val': { label: 'Nachtelijke load: validatie', gloss: 'Waar wijzigingen nagekeken worden.' },
  'airflow-prd': { label: 'Nachtelijke load: productie', gloss: 'Wat de business gebruikt.' },
  mailpit: { label: 'Testmailbox' },
}
const BASELINE_WORD = { ready: 'Klaar', preparing: 'Wordt nagekeken', missing: 'Nog niet nagekeken', failed: 'Kon niet klaargezet worden' } as const
const PROGRESS: Record<string, StatusState> = { todo: 'idle', running: 'running', done: 'ok', failed: 'bad' }

/** Everything behind "Details": the services, the practice system, the data switch, what is left over, the overview. */
export function StatusDetails() {
  const { server, catalog, actions } = useStudio()
  if (!server) return null
  return (
    <div className={styles.grid}>
      <section aria-labelledby="svc-h">
        <h2 id="svc-h" className={styles.h}>
          Alles wat de demo's nodig hebben
        </h2>
        <p className={styles.sub}>Groen betekent klaar.</p>
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
      <section className={styles.side} aria-label="Meer">
        <Toggle
          label="Oefendata blijft binnenkomen"
          checked={server.feeder.running}
          onChange={(v) => void actions.setFeeder(v)}
          disabled={!!server.job}
          help={
            server.job
              ? 'Je kan dit wijzigen zodra het huidige werk klaar is.'
              : server.feeder.running
                ? `Verzonnen bestellingen en leveringen komen elke minuut binnen. Zet dit uit om de cijfers te bevriezen.${server.feeder.simulated_day ? ` In het oefenbedrijf is het nu ${formatDay(server.feeder.simulated_day, 'noyear')}.` : ''}`
                : 'Bevroren. De cijfers blijven zoals ze zijn.'
          }
        />
        <LeftoversNotice />
        <a href={catalog?.overview_url || OVERVIEW_URL} target="_blank" rel="noreferrer" className={styles.link} title="Het eigen dashboard van de presentator">
          Open het systeemoverzicht (nieuw tabblad) <ExternalIcon size="0.9em" />
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
        <span className={styles.name}>Het oefensysteem</span>
        <span className={styles.word}>{BASELINE_WORD[baseline.status]}</span>
        {baseline.status !== 'ready' && baseline.detail && <span className={styles.detail}>{envWords(baseline.detail)}</span>}
      </p>
      {show && (
        <ol className={styles.progress} aria-label="Checks van het oefensysteem">
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
