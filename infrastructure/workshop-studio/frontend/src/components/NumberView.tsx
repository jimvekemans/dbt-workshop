import type { NumberData } from '../api/types'
import { sampleTrail } from '../api/adapt'
import { formatNumber } from '../lib/format'
import styles from './NumberView.module.css'

/** `trail`: the values this card showed earlier while the step ran (oldest first, the current one last). */
export function NumberView({ data, trail }: { data: NumberData; trail?: number[] }) {
  const value = data.value
  const other = data.compare?.value
  const known = data.compare && value !== null && other !== null && other !== undefined
  return (
    <div className={styles.wrap}>
      <p className={styles.big}>
        <span className={styles.value} key={String(value)}>
          {value === null ? '–' : formatNumber(value, 2)}
        </span>
        {data.unit && <span className={styles.unit}>{data.unit}</span>}
      </p>
      <p className={styles.compare}>
        {data.compare && known ? (
          <>
            <span>{data.compare.label}:</span>
            <b className={styles.second}>{formatNumber(other as number, 2)}</b>
            {data.unit && <span>{data.unit}</span>}
            {value === other && <span className={styles.same}>(hetzelfde)</span>}
          </>
        ) : data.compare ? (
          `${data.compare.label}: nog niet gekend`
        ) : (
          ' '
        )}
      </p>
      {trail && trail.length >= 3 && (
        <p className={styles.trail}>
          <span>Het toonde: </span>
          {sampleTrail(trail).map((v, i) => (
            <span key={i} className={styles.step}>
              {i > 0 && <span aria-hidden="true"> → </span>}
              {i > 0 && <span className="sr-only"> daarna </span>}
              {formatNumber(v, 2)}
            </span>
          ))}
        </p>
      )}
    </div>
  )
}
