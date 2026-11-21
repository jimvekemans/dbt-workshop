import { useEffect, useId, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import type { CodeBlockData } from '../api/types'
import { highlightLines } from '../lib/highlight'
import { plainText } from '../lib/glossary'
import { baseName, indentOf, segments } from '../lib/code'
import { RichText } from './Term'
import { StatusBadge } from './StatusBadge'
import { ExternalIcon } from './icons'
import styles from './CodeBlock.module.css'

/**
 * Code with its notes in plain words, each one shown once: right under the line it explains, with the same number badge.
 * A noted line and its note light up together. Long runs of lines without a note are folded behind "Toon regels 8-14".
 * Wide screens scroll long lines sideways (a fade at the edge says so); narrow ones wrap them.
 */
export function CodeBlock({ block }: { block: CodeBlockData }) {
  const uid = useId()
  const html = useMemo(() => highlightLines(block.lines.map((l) => l.text), block.language), [block])
  const [active, setActive] = useState<number | null>(null)
  const [opened, setOpened] = useState<ReadonlySet<number>>(new Set())
  const grid = useRef<HTMLDivElement>(null)
  const [more, setMore] = useState(false)

  // is there more to the right? (the fade at the edge shows only then)
  useEffect(() => {
    const el = grid.current
    if (!el) return
    const check = () => setMore(el.scrollWidth > el.clientWidth + el.scrollLeft + 2)
    check()
    el.addEventListener('scroll', check, { passive: true })
    if (typeof ResizeObserver === 'undefined') return () => el.removeEventListener('scroll', check)
    const ro = new ResizeObserver(check)
    ro.observe(el)
    return () => {
      el.removeEventListener('scroll', check)
      ro.disconnect()
    }
  }, [opened, block])

  const noteOf: Record<number, number> = {}
  let count = 0
  block.lines.forEach((l, i) => {
    if (l.note) noteOf[i] = ++count
  })
  const unlight = (e: { currentTarget: HTMLElement }) => {
    if (!e.currentTarget.contains(document.activeElement)) setActive(null)
  }
  const range = (start: number, end: number) => {
    const a = block.lines[start]?.n
    const b = block.lines[end - 1]?.n
    return a === b ? `regel ${a}` : `regels ${a}–${b}`
  }

  const segs = segments(block.lines, opened)

  return (
    <figure className={styles.block}>
      <figcaption className={styles.caption}>
        <span className={styles.fileInfo}>
          <span className={styles.fileLabel}>{block.file_label ? <RichText text={block.file_label} /> : baseName(block.file)}</span>
          <code className={styles.file}>{block.file}</code>
        </span>
        {block.status && <StatusBadge status={block.status} />}
        {block.gitlab_url && (
          <a className={styles.open} href={block.gitlab_url} target="_blank" rel="noreferrer">
            Openen in GitLab <ExternalIcon size="0.9em" aria-hidden="true" />
          </a>
        )}
      </figcaption>

      <div className={styles.scrollWrap} data-more={more ? '' : undefined}>
        <div ref={grid} className={styles.grid} role="group" tabIndex={0} aria-label={`Code uit ${plainText(block.file_label) || baseName(block.file)}`}>
          {segs.map((s, r) => {
            if (s.kind === 'gap' || s.kind === 'hide') {
              const n = s.end - s.start
              const opening = s.kind === 'gap'
              return (
                <div key={`${s.kind}${s.start}`} className={styles.gap}>
                  <button
                    type="button"
                    className={styles.more}
                    aria-expanded={!opening}
                    onClick={() =>
                      setOpened((o) => {
                        const x = new Set(o)
                        if (opening) x.add(s.start)
                        else x.delete(s.start)
                        return x
                      })
                    }
                  >
                    {opening ? `Toon ${range(s.start, s.end)} (nog ${n})` : `Verberg ${range(s.start, s.end)}`}
                  </button>
                </div>
              )
            }
            const l = block.lines[s.i]!
            const k = noteOf[s.i]
            const on = k !== undefined && active === k
            const ind = indentOf(l.text)
            return (
              <div key={l.n} className={styles.row} data-noted={k ? '' : undefined} data-active={on ? '' : undefined} data-note={k} style={{ '--r': r + 1 } as CSSProperties}>
                <span className={styles.num} aria-hidden="true">
                  {l.n}
                </span>
                <span className={styles.mark} aria-hidden="true">
                  {k && <span className={styles.marker}>{k}</span>}
                </span>
                <code
                  className={styles.text}
                  style={{ '--ind': `${ind}ch` } as CSSProperties}
                  tabIndex={k ? 0 : undefined}
                  aria-describedby={k ? `${uid}-n${k}` : undefined}
                  onFocus={k ? () => setActive(k) : undefined}
                  onMouseEnter={k ? () => setActive(k) : undefined}
                  onBlur={k ? unlight : undefined}
                  onMouseLeave={k ? unlight : undefined}
                >
                  <span dangerouslySetInnerHTML={{ __html: html[s.i] || ' ' }} />
                </code>
                {l.note && (
                  <div
                    id={`${uid}-n${k}`}
                    role="note"
                    className={styles.inlineNote}
                    onMouseEnter={() => setActive(k!)}
                    onMouseLeave={unlight}
                  >
                    <b className={styles.badge} aria-hidden="true">
                      {k}
                    </b>
                    <span className="sr-only">Opmerking {k}: </span>
                    <span className={styles.noteText}>
                      <RichText text={l.note} />
                    </span>
                  </div>
                )}
              </div>
            )
          })}
        </div>
      </div>
    </figure>
  )
}
