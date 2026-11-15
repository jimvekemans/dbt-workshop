export type GlossarySegment = { kind: 'text'; text: string } | { kind: 'term'; term: string; label: string }

const TERM = /\[\[([^\]|]+?)(?:\|([^\]]+?))?\]\]/g

/** Splits "a [[term]] b" and "a [[term|shown words]] b" into text and term segments. */
export function parseGlossary(text: string): GlossarySegment[] {
  const out: GlossarySegment[] = []
  let last = 0
  for (const m of text.matchAll(TERM)) {
    const start = m.index ?? 0
    if (start > last) out.push({ kind: 'text', text: text.slice(last, start) })
    const term = (m[1] ?? '').trim()
    out.push({ kind: 'term', term, label: (m[2] ?? m[1] ?? '').trim() })
    last = start + m[0].length
  }
  if (last < text.length) out.push({ kind: 'text', text: text.slice(last) })
  return out
}

/** Looks a term up case-insensitively. */
export function lookupTerm(glossary: Record<string, string>, term: string): string | undefined {
  if (term in glossary) return glossary[term]
  const lower = term.toLowerCase()
  for (const k of Object.keys(glossary)) if (k.toLowerCase() === lower) return glossary[k]
  return undefined
}

export function stripTerms(text: string): string {
  return parseGlossary(text)
    .map((s) => (s.kind === 'text' ? s.text : s.label))
    .join('')
}

/** The text with every [[term]] replaced by the words it shows (for places that cannot hold a button). */
export function plainText(text: string): string {
  return parseGlossary(text)
    .map((s) => (s.kind === 'text' ? s.text : (s.label ?? s.term)))
    .join('')
}
