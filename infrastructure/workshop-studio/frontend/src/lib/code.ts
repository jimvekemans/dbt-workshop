import type { CodeLine } from '../api/types'

export type Segment = { kind: 'line'; i: number } | { kind: 'gap'; start: number; end: number } | { kind: 'hide'; start: number; end: number }

/** Runs of this many un-noted lines or more are folded away behind a button. */
export const FOLD_AT = 4

/** The lines to show: every line, except long runs of lines without a note (unless that run was opened). */
export function segments(lines: CodeLine[], opened: ReadonlySet<number>): Segment[] {
  const out: Segment[] = []
  if (!lines.some((l) => l.note)) return lines.map((_, i) => ({ kind: 'line', i }))
  let i = 0
  while (i < lines.length) {
    if (lines[i]?.note) {
      out.push({ kind: 'line', i })
      i += 1
      continue
    }
    let j = i
    while (j < lines.length && !lines[j]?.note) j += 1
    if (j - i >= FOLD_AT && !opened.has(i)) out.push({ kind: 'gap', start: i, end: j })
    else {
      for (let k = i; k < j; k += 1) out.push({ kind: 'line', i: k })
      // a run that was opened can be closed again
      if (j - i >= FOLD_AT) out.push({ kind: 'hide', start: i, end: j })
    }
    i = j
  }
  return out
}

/** Spaces at the start of a line (a tab counts as two): wrapped pieces of a long line hang under this indent. */
export function indentOf(text: string): number {
  const lead = /^[ \t]*/.exec(text)?.[0] ?? ''
  return lead.replace(/\t/g, '  ').length
}

export const baseName = (file: string): string => file.split('/').filter(Boolean).pop() ?? file
