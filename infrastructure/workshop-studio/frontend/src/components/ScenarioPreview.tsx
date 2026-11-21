import { useLayoutEffect, useRef, useState } from 'react'
import type { CatalogScenario } from '../api/types'
import { previewIdea } from '../lib/visual'
import { PlainTerms, RichText } from './Term'
import { StatusBadge, isFeatureStatus } from './StatusBadge'
import { VisualView } from './VisualView'
import styles from './ScenarioPreview.module.css'

/** Does the tile have anything to preview? (An old backend sends no idea.) */
export function hasPreview(s: CatalogScenario): boolean {
  return previewIdea(s.idea) !== null
}

/**
 * What a home-page tile shows while the presenter points at it: the idea of the scenario in short, its headline above
 * the small schematic. A flow also lists its stages under the two status labels. It hangs from the tile (below it, or
 * above it when the window has no room below, and flush right in the last column) and is part of the tile: a click on
 * it opens the scenario like a click on the tile. It is only a picture, so it is hidden from screen readers, out of the
 * tab order, and [[terms]] in it are plain words.
 */
export function ScenarioPreview({ s, href }: { s: CatalogScenario; href: string }) {
  const idea = previewIdea(s.idea)
  const ref = useRef<HTMLAnchorElement>(null)
  const [place, setPlace] = useState<{ above: boolean; right: boolean }>({ above: false, right: false })
  // before the first paint: below the tile if it fits in the window, else above it when that has more room; at the right edge when it would run out of the window
  useLayoutEffect(() => {
    const el = ref.current
    const tile = el?.parentElement
    if (!el || !tile) return
    const t = tile.getBoundingClientRect()
    const need = el.offsetHeight + 16
    const below = window.innerHeight - t.bottom
    const above = t.top - 72
    const next = { above: below < need && above > below, right: t.left + el.offsetWidth > window.innerWidth - 8 && t.right - el.offsetWidth >= 8 }
    setPlace((p) => (p.above === next.above && p.right === next.right ? p : next))
  }, [])
  if (!idea) return null
  // a flow's stages, grouped under the two labels (each label once, so a long flow does not crowd the drawer)
  const stages = s.kind === 'flow' ? (s.steps ?? []) : []
  const groups = (['today', 'next'] as const)
    .map((status) => ({ status, names: stages.filter((x) => x.status === status && isFeatureStatus(x.status)).map((x) => x.feature || x.title) }))
    .filter((g) => g.names.length > 0)
  return (
    <PlainTerms>
      <a ref={ref} className={styles.preview} href={href} tabIndex={-1} aria-hidden="true" data-preview="" data-above={place.above ? '' : undefined} data-right={place.right ? '' : undefined}>
        <p className={styles.kicker}>Het idee</p>
        {idea.headline && (
          <p className={styles.headline}>
            <RichText text={idea.headline} />
          </p>
        )}
        <VisualView visual={idea.visual} compact />
        {groups.length > 0 && (
          <div className={styles.stages} data-stages="">
            {groups.map((g) => (
              <p key={g.status}>
                <StatusBadge status={g.status} />
                <span><RichText text={g.names.join(' · ')} /></span>
              </p>
            ))}
          </div>
        )}
      </a>
    </PlainTerms>
  )
}
