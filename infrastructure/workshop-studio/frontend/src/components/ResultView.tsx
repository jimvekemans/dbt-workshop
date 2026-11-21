import type { ResultData, ResultReason } from '../api/types'
import { Status } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './ResultView.module.css'

/** The summary line of the step's last action. `data` carries facts; refused or failed ones are marked as stopped. */
export function ResultView({ data }: { data: ResultData }) {
  if (!data.summary) return <EmptyNote>Het antwoord verschijnt zodra de stap klaar is.</EmptyNote>
  const facts = data.data ?? {}
  const stopped = facts.refused === true || facts.state === 'failed'
  const raw = Array.isArray(facts.reasons) ? facts.reasons : []
  const reasons = raw.filter((r): r is string => typeof r === 'string')
  const rows = raw.filter((r): r is ResultReason => !!r && typeof r === 'object' && typeof (r as ResultReason).label === 'string')
  // the action names the two sides ("Toolbox" / "Productiepackage"); a side nobody named is first / second
  const title = (key: string, fallback: string) => (typeof facts[key] === 'string' && facts[key] ? (facts[key] as string) : fallback)
  const leftTitle = title('left_title', 'Eerste')
  const rightTitle = title('right_title', 'Tweede')
  const labelTitle = title('label_title', 'Tool') // what the rows are about, when it is not tools
  const twoSides = rows.some((r) => r.right !== undefined && r.right !== null)
  return (
    <div className={styles.wrap} data-state={stopped ? 'bad' : 'ok'}>
      <Status state={stopped ? 'bad' : 'ok'}>{stopped ? 'Dit werd gestopt' : 'Dit lukte'}</Status>
      <p className={styles.text}>{data.summary}</p>
      {reasons.length > 0 && (
        <ul className={styles.reasons}>
          {reasons.map((r, i) => (
            <li key={i}>{r}</li>
          ))}
        </ul>
      )}
      {rows.length > 0 && (
        <table className={styles.compare}>
          <thead>
            <tr>
              <th scope="col">{labelTitle}</th>
              <th scope="col">{leftTitle}</th>
              {twoSides && <th scope="col">{rightTitle}</th>}
              <th scope="col" className="sr-only">Vergelijking</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => {
              const two = r.right !== undefined && r.right !== null
              return (
                <tr key={i}>
                  <th scope="row" data-label={labelTitle}>{r.label}</th>
                  <td data-label={leftTitle}>{r.left ?? '–'}</td>
                  {twoSides && <td data-label={rightTitle}>{two ? r.right : 'niet inbegrepen'}</td>}
                  <td>
                    {two ? (
                      r.same ? <span className={styles.same}>✓ hetzelfde</span> : <span className={styles.diff}>anders</span>
                    ) : (
                      twoSides && <span className={styles.diff}>enkel in {leftTitle.toLowerCase()}</span>
                    )}
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      )}
    </div>
  )
}
