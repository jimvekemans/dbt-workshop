import { useCallback, useSyncExternalStore } from 'react'

/**
 * The presenter's switch "Technical names": the small technical lines (`tech` of a scenario and of a step) are shown
 * or hidden everywhere. On by default; the choice is kept in this browser (like the text size) and shared by every
 * component through one small store, so the switch in the top bar and the lines on the page always agree.
 */
export const TECH_KEY = 'studio-tech-names'

let fallback: boolean | null = null // only used when localStorage cannot be read or written (private window)
const listeners = new Set<() => void>()

function read(): boolean {
  try {
    const v = localStorage.getItem(TECH_KEY)
    return v !== 'off'
  } catch {
    return fallback ?? true
  }
}

function write(on: boolean) {
  fallback = on
  try {
    localStorage.setItem(TECH_KEY, on ? 'on' : 'off')
  } catch {
    /* private window: the choice just is not remembered */
  }
  listeners.forEach((l) => l())
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  const onStorage = (e: StorageEvent) => e.key === TECH_KEY && listener()
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

export function useTechNames() {
  const on = useSyncExternalStore(subscribe, read, () => true)
  const toggle = useCallback(() => write(!read()), [])
  return { on, toggle }
}

/** The line shown for a list of keywords: "vdp_ci · svc_ci_dbt". Empty entries are skipped. */
export function techText(tech: readonly string[] | string | null | undefined): string {
  if (!tech) return ''
  const items = typeof tech === 'string' ? [tech] : tech
  return items.map((t) => t.trim()).filter(Boolean).join(' · ')
}
