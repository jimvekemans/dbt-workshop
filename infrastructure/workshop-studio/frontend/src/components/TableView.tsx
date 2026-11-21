import type { Cell, LiveEvidence, TableData } from '../api/types'
import { formatNumber } from '../lib/format'
import { StatusMark } from './Status'
import { EmptyNote } from './EvidenceFrame'
import styles from './TableView.module.css'

function cell(v: Cell | undefined): string {
  if (v === null || v === undefined) return '–'
  if (typeof v === 'number') return formatNumber(v, 2)
  if (typeof v === 'boolean') return v ? 'Ja' : 'Nee'
  return String(v)
}

/** Rows scroll inside their own frame (the header stays put), so a 200-row answer never pushes the page around. */
export function TableView({ item, data, title }: { item: LiveEvidence; data: TableData; title: string }) {
  if (data.rows.length === 0) return <EmptyNote>{data.empty || 'Nog niets om te tonen.'}</EmptyNote>
  const footer = [item.footer, data.footer].find((f): f is string => typeof f === 'string' && f.trim() !== '')
  const flagged = !!item.highlight || data.rows.some((r) => r.state)
  return (
    <>
      <div className={styles.scroll} role="group" aria-label={title} tabIndex={0}>
        <table className={styles.table}>
          <thead>
            <tr>
              {flagged && (
                <th scope="col" className={styles.flag}>
                  <span className="sr-only">Check</span>
                </th>
              )}
              {data.columns.map((c) => (
                <th key={c.key} scope="col" className={data.rows.some((r) => typeof r.cells[c.key] === 'number') ? styles.num : undefined}>
                  {c.unit && !c.label.includes(c.unit) ? `${c.label} (${c.unit})` : c.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.rows.map((r, i) => {
              const state = r.state === 'bad' ? 'bad' : r.state === 'good' ? 'ok' : undefined
              return (
                <tr key={i} data-state={state} className={state ? styles.hit : undefined}>
                  {flagged && (
                    <td className={styles.flag}>
                      {state ? (
                        <>
                          <StatusMark state={state} />
                          <span className="sr-only">{state === 'bad' ? 'Probleem' : 'In orde'}</span>
                        </>
                      ) : null}
                    </td>
                  )}
                  {data.columns.map((c) => (
                    <td key={c.key} className={typeof r.cells[c.key] === 'number' ? styles.num : cell(r.cells[c.key]).length <= 20 ? styles.keep : styles.long}>
                      {cell(r.cells[c.key])}
                    </td>
                  ))}
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      {footer && <p className={styles.footer}>{footer}</p>}
      {data.truncated && <p className={styles.more}>Enkel de eerste {formatNumber(data.rows.length)} rijen worden getoond.</p>}
    </>
  )
}
