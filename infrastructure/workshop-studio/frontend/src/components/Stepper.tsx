import { cx } from '../lib/cx'
import styles from './Stepper.module.css'

export interface StepperPart {
  id: string
  name: string
}
interface Props {
  parts: StepperPart[]
  current: string
  onSelect: (id: string) => void
  label?: string
}

export function Stepper({ parts, current, onSelect, label = 'Delen van dit scenario' }: Props) {
  const ci = parts.findIndex((p) => p.id === current)
  return (
    <nav aria-label={label} className={styles.nav}>
      <ol className={styles.list}>
        {parts.map((p, i) => {
          const state = i < ci ? 'done' : i === ci ? 'current' : 'todo'
          return (
            <li key={p.id} className={cx(styles.item, styles[state])}>
              <button
                type="button"
                className={styles.button}
                aria-current={state === 'current' ? 'step' : undefined}
                onClick={() => onSelect(p.id)}
              >
                <span className={styles.num} aria-hidden="true">
                  {state === 'done' ? '✓' : i + 1}
                </span>
                <span className={styles.name}>
                  <span className="sr-only">Deel {i + 1}: </span>
                  {p.name}
                  {state === 'done' && <span className="sr-only"> (klaar)</span>}
                  {state === 'current' && <span className="sr-only"> (je bent hier)</span>}
                </span>
              </button>
            </li>
          )
        })}
      </ol>
      <p className={styles.phoneName} aria-hidden="true">
        Deel {ci + 1} van {parts.length} · {parts[ci]?.name}
      </p>
      <p className={styles.keys} aria-hidden="true">
        ← → toetsen wisselen tussen de delen
      </p>
    </nav>
  )
}
