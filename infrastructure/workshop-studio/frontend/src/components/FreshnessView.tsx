import type { FreshnessData, FreshnessState } from '../api/types'
import { Status } from './Status'
import type { StatusState } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './FreshnessView.module.css'

const MAP: Record<FreshnessState, StatusState> = { fresh: 'ok', late: 'warn', very_late: 'bad', unknown: 'idle' }

export function FreshnessView({ data }: { data: FreshnessData }) {
  if (data.sources.length === 0) return <EmptyNote>{data.empty || 'No delivery has been checked yet.'}</EmptyNote>
  return (
    <ul className={styles.list}>
      {data.sources.map((s) => {
        const state = MAP[s.state] ?? 'idle'
        return (
          <li key={s.label} className={styles.item} data-state={state}>
            <span className={styles.label}>{s.label}</span>
            <span className={styles.age}>{s.last_data ? `last data ${s.last_data}` : ''}</span>
            <Status state={state}>{s.state_text}</Status>
          </li>
        )
      })}
    </ul>
  )
}
