import { useSyncExternalStore } from 'react'
import type { PartId } from '../api/types'

export const PARTS: { id: PartId; name: string }[] = [
  { id: 'problem', name: 'What goes wrong' },
  { id: 'idea', name: 'The idea' },
  { id: 'code', name: 'The code' },
  { id: 'demo', name: 'Live demo' },
]

/** The one way a part is written everywhere (stepper, page heading, Back / Next): "Part 2 · The idea". */
export function partTitle(id: PartId): string {
  const i = PARTS.findIndex((p) => p.id === id)
  return `Part ${i + 1} · ${PARTS[i]?.name ?? ''}`
}

export type Route =
  | { name: 'home' }
  /** A hash that is no page of this app: the app shows the start page and says so. */
  | { name: 'unknown' }
  /** `repair`: the part in the link was missing or wrong, so the address should be replaced by the part-1 address. `from`: the scenario the visitor came from (a related link). */
  | { name: 'scenario'; id: string; part: PartId; repair?: true; from?: string }

export function isPart(x: string | undefined): x is PartId {
  return x === 'problem' || x === 'idea' || x === 'code' || x === 'demo'
}

export function parseHash(hash: string): Route {
  const [path = '', query = ''] = hash.replace(/^#\/?/, '').split('?')
  const seg = path.split('/').filter(Boolean)
  if (seg.length === 0) return { name: 'home' }
  if (seg[0] === 's' && seg[1]) {
    const part = seg[2]
    const from = new URLSearchParams(query).get('from')
    return {
      name: 'scenario',
      id: decodeURIComponent(seg[1]),
      part: isPart(part) ? part : 'problem',
      ...(isPart(part) ? {} : { repair: true as const }),
      ...(from ? { from } : {}),
    }
  }
  return { name: 'unknown' }
}

export function toHash(route: Route): string {
  if (route.name !== 'scenario') return '#/'
  return `#/s/${encodeURIComponent(route.id)}/${route.part}`
}

export function navigate(route: Route): void {
  const next = toHash(route)
  if (window.location.hash !== next) window.location.hash = next
}

export function partIndex(part: PartId): number {
  return PARTS.findIndex((p) => p.id === part)
}

function subscribe(cb: () => void): () => void {
  window.addEventListener('hashchange', cb)
  return () => window.removeEventListener('hashchange', cb)
}
const snapshot = () => window.location.hash

export function useRoute(): Route {
  const hash = useSyncExternalStore(subscribe, snapshot, () => '')
  return parseHash(hash)
}
