import { useId, useMemo, useState } from 'react'
import type { BarsData } from '../api/types'
import { barsModel } from '../api/adapt'
import { formatDay, formatNumber } from '../lib/format'
import { EmptyNote } from './EvidenceFrame'
import styles from './BarsView.module.css'

type Role = 'good' | 'bad' | 'other'
const W = 720
const H = 308
const M = { l: 56, r: 10, t: 34, b: 54 }

/** A day label like 2026-11-04 reads as "4 Nov" on the axis (the readout and the table keep the whole date). */
function axisLabel(l: string): string {
  l = l.replace(/\s*\(today\)$/i, '')
  return /^\d{4}-\d{2}-\d{2}$/.test(l) ? formatDay(l, 'short').replace(/^\w+ /, '') : l
}

function niceMax(v: number): number {
  if (v <= 0) return 1
  const p = 10 ** Math.floor(Math.log10(v))
  for (const m of [1, 2, 2.5, 5, 10]) if (v <= m * p) return m * p
  return 10 * p
}

export function BarsView({ data, title, slots, unit }: { data: BarsData; title: string; slots?: string[]; unit?: string }) {
  const uid = useId().replace(/:/g, '')
  const [active, setActive] = useState<number | null>(null)

  const model = useMemo(() => barsModel(data), [data])
  if (model.labels.length === 0 || model.series.length === 0) return <EmptyNote>No numbers yet.</EmptyNote>

  const { labels, today } = model
  const isToday = (i: number) => !!today[i]
  // slots that have no data yet are drawn as dashed outlines, so bars never re-space while results arrive
  const all = slots && slots.length > labels.length && labels.every((l, i) => slots[i] === l) ? slots : labels
  const series = model.series.map((s) => s.name)
  const get = (label: string, s: string) => model.series.find((x) => x.name === s)?.values[labels.indexOf(label)] ?? 0
  const grouped = false as boolean
  const role = (s: string): Role => {
    const st = model.series.find((x) => x.name === s)?.state
    return st === 'good' ? 'good' : st === 'bad' ? 'bad' : 'other'
  }
  const totals = all.map((_, i) => (i < labels.length ? model.total(i) : 0))
  const peak = grouped ? Math.max(...labels.flatMap((l) => series.map((s) => get(l, s)))) : Math.max(...totals)
  const top = niceMax(peak)
  const plotW = W - M.l - M.r
  const plotH = H - M.t - M.b
  const band = plotW / all.length
  const barW = Math.min(64, band * 0.64)
  const yOf = (v: number) => M.t + plotH - (v / top) * plotH
  const showValues = all.length <= 16
  const skip = Math.ceil(all.length / 14)
  const fillOf = (s: string, i: number) =>
    role(s) === 'good' ? 'var(--chart-good)' : role(s) === 'bad' ? `url(#${uid}-hatch)` : i % 2 ? 'var(--chart-neutral-2)' : 'var(--chart-neutral)'

  const describe = (li: number) => {
    const l = labels[li] ?? ''
    const u = unit ? ` ${unit}` : ''
    return `${l}${isToday(li) ? ' (the day is not over yet)' : ''}: ${series.map((s) => `${s} ${formatNumber(get(l, s))}${u}`).join(', ')}${series.length > 1 ? ` (total ${formatNumber(totals[li] ?? 0)}${u})` : ''}`
  }
  const readout = active === null ? 'Point at a bar to read its numbers. Every bar also shows its number.' : describe(active)

  return (
    <div className={styles.wrap}>
      {unit && <p className={styles.counting}>Each bar counts {unit}.</p>}
      <ul className={styles.legend} aria-label="What the colours mean">
        {series.map((s, i) => (
          <li key={s}>
            <svg width="22" height="16" aria-hidden="true">
              <rect width="22" height="16" rx="3" fill={fillOf(s, i)} />
              {role(s) === 'bad' && <rect width="22" height="16" rx="3" fill="none" stroke="var(--chart-bad)" strokeWidth="2" />}
            </svg>
            <span>
              {s}
              {role(s) !== 'other' && <span className={styles.role}> ({role(s) === 'good' ? 'as it should be' : 'a problem'})</span>}
            </span>
          </li>
        ))}
        {today.some(Boolean) && (
          <li>
            <svg width="22" height="16" aria-hidden="true">
              <rect x="1" y="1" width="20" height="14" rx="3" fill="var(--chart-good)" fillOpacity="0.6" stroke="var(--ink-2)" strokeWidth="1.5" strokeDasharray="4 3" />
            </svg>
            <span>dashed: today, not over yet</span>
          </li>
        )}
      </ul>
      <svg className={styles.svg} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${title}. ${labels.length} bars. See the table below the chart for every number.`}>
        <defs>
          <pattern id={`${uid}-hatch`} width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
            <rect width="7" height="7" fill="var(--chart-bad)" />
            <rect width="3" height="7" fill="var(--surface)" fillOpacity="0.55" />
          </pattern>
        </defs>
        {[0, 0.25, 0.5, 0.75, 1].map((f) => (
          <g key={f}>
            <line x1={M.l} x2={W - M.r} y1={yOf(top * f)} y2={yOf(top * f)} stroke="var(--chart-grid)" strokeWidth="1" />
            <text x={M.l - 8} y={yOf(top * f) + 4} textAnchor="end" className={styles.axis}>
              {formatNumber(top * f)}
            </text>
          </g>
        ))}
        {all.map((l, li) => {
          const cx = M.l + band * li + band / 2
          let acc = 0
          const n = series.length
          return (
            <g key={`${l}-${li}`} onMouseEnter={() => setActive(li)} onMouseLeave={() => setActive(null)} className={active === li ? styles.on : undefined}>
              <rect x={M.l + band * li} y={M.t} width={band} height={plotH} fill="transparent" />
              {li >= labels.length && <rect x={cx - barW / 2} y={M.t} width={barW} height={plotH} rx="3" fill="none" stroke="var(--line-strong)" strokeWidth="1.5" strokeDasharray="4 4" />}
              {series.map((s, si) => {
                const v = get(l, s)
                const w = grouped ? barW / n : barW
                const x = grouped ? cx - barW / 2 + si * w : cx - barW / 2
                const base = grouped ? 0 : acc
                const y = yOf(base + v)
                const h = Math.max(0, yOf(base) - y)
                acc += v
                return (
                  <g key={s}>
                    <rect
                      className={styles.bar}
                      x={x}
                      y={y}
                      width={w - (grouped ? 2 : 0)}
                      height={h}
                      rx="2"
                      fill={fillOf(s, si)}
                      fillOpacity={isToday(li) ? 0.6 : undefined}
                      stroke={isToday(li) ? 'var(--ink-2)' : undefined}
                      strokeWidth={isToday(li) ? 1.5 : undefined}
                      strokeDasharray={isToday(li) ? '4 3' : undefined}
                      style={{ '--y': y, '--h': h } as React.CSSProperties}
                    >
                      <title>{`${l}: ${s} ${formatNumber(v)}`}</title>
                    </rect>
                    {showValues && !grouped && h > 20 && w > 26 && series.filter((x) => get(l, x) > 0).length > 1 && v > 0 && (
                      <text x={x + w / 2} y={y + h / 2 + 4} textAnchor="middle" className={styles.inside} style={{ fill: role(s) === 'bad' ? 'var(--ink)' : '#fff' }}>
                        {formatNumber(v)}
                      </text>
                    )}
                    {showValues && grouped && v > 0 && (
                      <text x={x + (w - 2) / 2} y={y - 5} textAnchor="middle" className={styles.value}>
                        {formatNumber(v)}
                      </text>
                    )}
                  </g>
                )
              })}
              {showValues && !grouped && (
                <text x={cx} y={yOf(totals[li] ?? 0) - 6} textAnchor="middle" className={styles.value}>
                  {formatNumber(totals[li] ?? 0)}
                </text>
              )}
              {li % skip === 0 && (
                <text x={cx} y={H - M.b + 18} textAnchor="middle" className={styles.axis}>
                  {axisLabel(l)}
                </text>
              )}
              {isToday(li) && (
                <g aria-hidden="true">
                  <line x1={cx} x2={cx} y1={H - M.b + 4} y2={H - M.b + 9} stroke="var(--ink-2)" strokeWidth="1.5" />
                  <text x={cx} y={H - M.b + 33} textAnchor="middle" className={styles.todayTag}>
                    today
                  </text>
                </g>
              )}
            </g>
          )
        })}
        {unit && (
          <text x={2} y={M.t - 18} textAnchor="start" className={styles.axisTitle}>
            {unit}
          </text>
        )}
        {data.x_label && (
          <text x={M.l + plotW / 2} y={H - 6} textAnchor="middle" className={styles.axisTitle}>
            {data.x_label}
          </text>
        )}
      </svg>
      <p className={styles.readout} aria-hidden="true">
        {readout}
      </p>
      <table className="sr-only">
        <caption>{title}{unit ? ` (${unit})` : ''}</caption>
        <thead>
          <tr>
            <th>{data.x_label ?? 'Item'}</th>
            {series.map((s) => (
              <th key={s}>{s}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {labels.map((l) => (
            <tr key={l}>
              <th>{l}</th>
              {series.map((s) => (
                <td key={s}>{formatNumber(get(l, s))}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
