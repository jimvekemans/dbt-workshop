import type { Catalog, FeatureStatus, Roadmap } from '../api/types'
import { isFeatureStatus } from '../components/StatusBadge'

export interface RoadmapRow {
  key: string
  feature: string
  status: FeatureStatus
  /** The scenario the row links to; rows of the file's `extras` have none. */
  scenarioId?: string
  /** `flow` rows link to the demo (that is where the stages are), the others to the start of the scenario. */
  part?: 'problem' | 'demo'
  note?: string | null
  /** Next steps (and any row): what goes wrong as long as this is not done. */
  why?: string | null
  /** For the presenter: the technical line of a flow stage, or the keywords of an ordinary scenario. */
  tech?: string | string[] | null
}
/** A flow (with its title) or a run of ordinary scenarios (no title) inside one group. */
export interface RoadmapBlock {
  key: string
  title: string | null
  scenarioId: string | null
  rows: RoadmapRow[]
}
export interface RoadmapGroup {
  id: string
  title: string
  blocks: RoadmapBlock[]
}
export interface RoadmapArea {
  area: string
  rows: RoadmapRow[]
}
export interface RoadmapSection {
  status: FeatureStatus
  /** The rows of this status: the groups in home-page order, then the extras by area. */
  groups: RoadmapGroup[]
  areas: RoadmapArea[]
  count: number
}
export interface RoadmapView {
  /** Always two, in the order of the talk: in place today, then the next steps. */
  sections: [RoadmapSection, RoadmapSection]
  today: number
  next: number
  total: number
}

/** The row of a flow stage or an ordinary scenario, or nothing when it has no feature or no known status. */
function rowOf(key: string, feature: string | null | undefined, status: string | null | undefined, why: string | null | undefined, rest: Partial<RoadmapRow>): RoadmapRow | null {
  if (!feature || !isFeatureStatus(status)) return null
  return { key, feature, status, ...(why ? { why } : {}), ...rest }
}

function sectionFor(status: FeatureStatus, catalog: Catalog | null, roadmap: Roadmap | null | undefined): RoadmapSection {
  const groups: RoadmapGroup[] = []
  for (const g of catalog?.groups ?? []) {
    const blocks: RoadmapBlock[] = []
    for (const s of g.scenarios) {
      if (s.kind === 'flow') {
        const rows = (s.steps ?? []).flatMap((st) => {
          const row = rowOf(`${s.id}/${st.id}`, st.feature, st.status, st.why, { scenarioId: s.id, part: 'demo', tech: st.tech })
          return row && row.status === status ? [row] : []
        })
        if (rows.length > 0) blocks.push({ key: s.id, title: s.title, scenarioId: s.id, rows })
      } else {
        const row = rowOf(s.id, s.feature, s.status, s.why, { scenarioId: s.id, part: 'problem', tech: s.tech })
        if (!row || row.status !== status) continue
        const last = blocks[blocks.length - 1]
        if (last && last.title === null) last.rows.push(row)
        else blocks.push({ key: `${g.id}/${s.id}`, title: null, scenarioId: null, rows: [row] })
      }
    }
    if (blocks.length > 0) groups.push({ id: g.id, title: g.title, blocks })
  }
  const areas: RoadmapArea[] = []
  ;(roadmap?.extras ?? []).forEach((x, i) => {
    if (x.status !== status) return
    let area = areas.find((a) => a.area === x.area)
    if (!area) areas.push((area = { area: x.area, rows: [] }))
    area.rows.push({ key: `extra-${i}`, feature: x.feature, status, note: x.note, ...(x.why ? { why: x.why } : {}) })
  })
  const count = groups.reduce((n, g) => n + g.blocks.reduce((m, b) => m + b.rows.length, 0), 0) + areas.reduce((n, a) => n + a.rows.length, 0)
  return { status, groups, areas, count }
}

/**
 * The closing overview, built from what the catalog already says, in two sections: what is in place today, then the
 * next steps. Inside a section the rows stay grouped as on the home page (the groups in order, each flow with its
 * stages, ordinary scenarios together); a flow or group without a row in the section is left out. The extra rows of
 * the roadmap file come last, grouped by area (in the order the areas first appear). A row without a known status is
 * left out altogether.
 */
export function buildRoadmap(catalog: Catalog | null, roadmap: Roadmap | null | undefined = catalog?.roadmap): RoadmapView {
  const today = sectionFor('today', catalog, roadmap)
  const next = sectionFor('next', catalog, roadmap)
  return { sections: [today, next], today: today.count, next: next.count, total: today.count + next.count }
}
