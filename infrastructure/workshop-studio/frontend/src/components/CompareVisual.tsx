import type { CompareSide, CompareVisual as Compare } from '../api/types'
import { visualTone } from '../lib/visual'
import type { Tone } from '../lib/visual'
import { StatusMark } from './Status'
import type { StatusState } from './Status'
import { RichText } from './Term'
import { VisualCaption } from './VisualCaption'
import styles from './CompareVisual.module.css'

const WORD: Record<Tone, string> = { ok: 'Beter', fix: 'Beter', bad: 'Probleem', warn: 'Opgelet', idle: 'Wacht' }

/** `kind` is the internal key; these are the words on screen. */
const KIND = { Without: 'Zonder', With: 'Met' } as const

function Side({ side, kind }: { side: CompareSide; kind: 'Without' | 'With' }) {
  const st = visualTone(side.state)
  return (
    <section className={styles.side} data-state={st} aria-label={`${KIND[kind]} vangnet`}>
      <p className={styles.kind}>{KIND[kind]} vangnet</p>
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

/** `compact`: the small version of the home-page preview (the two titles with their state, no lists). */
export function CompareVisual({ visual, compact }: { visual: Compare; compact?: boolean }) {
  return (
    <figure className={styles.figure} data-compact={compact ? '' : undefined}>
      <div className={styles.pair}>
        <Side side={visual.left} kind="Without" />
        <Side side={visual.right} kind="With" />
      </div>
      <VisualCaption text={visual.caption} />
    </figure>
  )
}
