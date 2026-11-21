import type { RelatedLink } from '../api/types'
import { useStudio } from '../lib/studio'
import { findCatalogScenario, neighbours } from '../lib/store'
import { plainText } from '../lib/glossary'
import { RichText } from './Term'
import styles from './Related.module.css'

/** A quiet "see also" block: related scenarios explain; they are never prerequisites. */
export function Related({ items, fromId }: { items: RelatedLink[]; fromId: string }) {
  const { catalog } = useStudio()
  const found = items.flatMap((r) => {
    const s = findCatalogScenario(catalog, r.id)
    return s ? [{ r, s, n: neighbours(catalog, s.id).index + 1 }] : []
  })
  if (found.length === 0) return null
  return (
    <aside className={styles.related} aria-labelledby="related-h">
      <h3 id="related-h" className={styles.title}>
        Verwant, niet verplicht:
      </h3>
      <ul className={styles.list}>
        {found.map(({ r, s, n }) => (
          <li key={s.id}>
            <a className={styles.link} href={`#/s/${encodeURIComponent(s.id)}/problem?from=${encodeURIComponent(fromId)}`}>
              Open scenario {String(n).padStart(2, '0')}: {plainText(s.title)} →
            </a>
            <span className={styles.why}>
              <RichText text={r.why} />
            </span>
          </li>
        ))}
      </ul>
    </aside>
  )
}
