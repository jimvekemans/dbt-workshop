import type { FlowNode, FlowVisual as Flow } from '../api/types'
import { visualTone } from '../lib/visual'
import { Status } from './Status'
import { RichText } from './Term'
import { VisualCaption } from './VisualCaption'
import styles from './FlowVisual.module.css'

export interface FlowLayout {
  /** Boxes that come one after the other; boxes in the same stage are alternatives (a fork that joins again). */
  stages: FlowNode[][]
  /** Links between neighbouring stages, as [stage index, position in stage] pairs. */
  edges: { from: [number, number]; to: [number, number] }[]
}

/**
 * Stages from the links: a box sits one stage after its furthest source, so two boxes with the same source (and the same
 * target) end up side by side. Without usable links the boxes simply follow one another, with no arrows.
 */
export function flowLayout(visual: Pick<Flow, 'nodes' | 'links'>): FlowLayout {
  const nodes = visual.nodes
  const idx = new Map(nodes.map((n, i) => [n.id, i]))
  const links = (visual.links ?? []).flatMap(([a, b]) => {
    const f = idx.get(a)
    const t = idx.get(b)
    return f === undefined || t === undefined || f === t ? [] : [[f, t] as [number, number]]
  })
  const rank = nodes.map(() => 0)
  if (links.length === 0) nodes.forEach((_, i) => (rank[i] = i))
  else {
    // longest path from a source; a cycle cannot loop forever
    for (let pass = 0; pass < nodes.length; pass += 1) {
      let moved = false
      for (const [f, t] of links) {
        if (rank[t]! < rank[f]! + 1) {
          rank[t] = rank[f]! + 1
          moved = true
        }
      }
      if (!moved) break
    }
    const linked = new Set(links.flat())
    nodes.forEach((_, i) => {
      if (!linked.has(i)) rank[i] = i > 0 ? rank[i - 1]! + 1 : 0
    })
  }
  const order = [...new Set(rank)].sort((a, b) => a - b)
  const stageOf = (i: number) => order.indexOf(rank[i]!)
  const stages: FlowNode[][] = order.map(() => [])
  const pos: number[] = []
  nodes.forEach((n, i) => {
    const s = stageOf(i)
    pos[i] = stages[s]!.length
    stages[s]!.push(n)
  })
  const edges = links.flatMap(([f, t]) => (stageOf(t) === stageOf(f) + 1 ? [{ from: [stageOf(f), pos[f]!] as [number, number], to: [stageOf(t), pos[t]!] as [number, number] }] : []))
  return { stages, edges }
}

const at = (p: number, count: number) => ((p + 0.5) / count) * 100

/** The arrows between two neighbouring stages: one line per link, one head per box that receives. Drawn for both directions; CSS shows one. */
function Connector({ from, to, edges, broken }: { from: FlowNode[]; to: FlowNode[]; edges: FlowLayout['edges']; broken: (a: FlowNode, b: FlowNode) => boolean }) {
  const heads = new Map<number, boolean>()
  const lines = edges.map((e) => {
    const bad = broken(from[e.from[1]]!, to[e.to[1]]!)
    heads.set(e.to[1], (heads.get(e.to[1]) ?? false) || bad)
    const ya = at(e.from[1], from.length)
    const yb = at(e.to[1], to.length)
    return { bad, ya, yb }
  })
  const style = (bad: boolean) => ({ stroke: bad ? 'var(--bad)' : 'currentColor', strokeDasharray: bad ? '4 5' : undefined })
  return (
    <span className={styles.link} aria-hidden="true">
      <svg className={styles.hLines} viewBox="0 0 48 100" preserveAspectRatio="none">
        {lines.map((l, i) => (
          <path key={i} d={`M0 ${l.ya} C 20 ${l.ya}, 20 ${l.yb}, 40 ${l.yb}`} style={style(l.bad)} />
        ))}
      </svg>
      <svg className={styles.vLines} viewBox="0 0 100 48" preserveAspectRatio="none">
        {lines.map((l, i) => (
          <path key={i} d={`M${l.ya} 0 C ${l.ya} 20, ${l.yb} 20, ${l.yb} 40`} style={style(l.bad)} />
        ))}
      </svg>
      {[...heads].map(([p, bad]) => (
        <span key={p} className={styles.head} style={{ '--p': at(p, to.length), color: bad ? 'var(--bad)' : undefined } as React.CSSProperties} />
      ))}
    </span>
  )
}

/** Boxes left to right, every one with an arrow in and an arrow out except the first and the last. A box that is fine says nothing; one may be the fix ("De oplossing").
 *  `compact`: the small version of the home-page preview (labels only, chips that wrap, no arrows drawn). */
export function FlowVisual({ visual, compact }: { visual: Flow; compact?: boolean }) {
  const { stages, edges } = flowLayout(visual)
  const broken = (a: FlowNode, b: FlowNode) => [visualTone(a.state), visualTone(b.state)].some((t) => t === 'bad' || t === 'idle')
  return (
    <figure className={styles.figure} data-compact={compact ? '' : undefined}>
      <ol className={styles.flow}>
        {stages.map((nodes, si) => {
          const next = stages[si + 1]
          const here = edges.filter((e) => e.from[0] === si)
          return (
            <li key={si} className={styles.stage} style={{ '--m': nodes.length } as React.CSSProperties} data-fork={nodes.length > 1 ? '' : undefined}>
              <div className={styles.nodes}>
                {nodes.map((n) => {
                  const st = visualTone(n.state)
                  return (
                    <div key={n.id} className={styles.node} data-state={st}>
                      {st === 'fix' && <span className={styles.fix}>De oplossing</span>}
                      {(st === 'bad' || st === 'warn' || st === 'idle') && <Status state={st} variant="plain" className={styles.state} />}
                      <span className={styles.label}><RichText text={n.label} /></span>
                      {n.note && (
                        <span className={styles.note}>
                          <RichText text={n.note} />
                        </span>
                      )}
                    </div>
                  )
                })}
              </div>
              {next && <Connector from={nodes} to={next} edges={here} broken={broken} />}
            </li>
          )
        })}
      </ol>
      <VisualCaption text={visual.caption} />
    </figure>
  )
}
