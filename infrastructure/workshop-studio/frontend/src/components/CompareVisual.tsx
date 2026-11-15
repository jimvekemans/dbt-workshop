import type { CompareSide, CompareVisual as Compare } from '../api/types'
import { visualTone } from '../lib/visual'
import type { Tone } from '../lib/visual'
import { StatusMark } from './Status'
import type { StatusState } from './Status'
import { RichText } from './Term'
import { VisualCaption } from './VisualCaption'
import styles from './CompareVisual.module.css'

const WORD: Record<Tone, string> = { ok: 'Better', fix: 'Better', bad: 'Problem', warn: 'Careful', idle: 'Waiting' }

function Side({ side, kind }: { side: CompareSide; kind: 'Without' | 'With' }) {
  const st = visualTone(side.state)
  return (
    <section className={styles.side} data-state={st} aria-label={`${kind} the safeguard`}>
      <p className={styles.kind}>{kind} the safeguard</p>
      <h3 className={styles.title}><RichText text={side.title} /></h3>
      <p className={styles.state}>
        <StatusMark state={(st === 'fix' ? 'ok' : st) as StatusState} />
        <span>{WORD[st]}</span>
      </p>
      <ul className={styles.items}>
        {side.items.map((t, i) => (
          <li key={i}>
            <RichText text={t} />
          </li>
        ))}
      </ul>
    </section>
  )
}

export function CompareVisual({ visual }: { visual: Compare }) {
  return (
    <figure className={styles.figure}>
      <div className={styles.pair}>
        <Side side={visual.left} kind="Without" />
        <Side side={visual.right} kind="With" />
      </div>
      <VisualCaption text={visual.caption} />
    </figure>
  )
}
