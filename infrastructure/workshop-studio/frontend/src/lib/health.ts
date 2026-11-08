import type { Health, Layer, NodeDef, NodeState, ScenarioSummary, StudioState, GroupId } from "../types";

export const HEALTH_RANK: Record<Health, number> = { idle: 0, ok: 1, warn: 2, fail: 3 };

export function worst(a: Health, b: Health): Health {
  return HEALTH_RANK[a] >= HEALTH_RANK[b] ? a : b;
}

/** Aggregate health of a set of nodes (idle only if everything is idle). */
export function aggregate(states: Record<string, NodeState>, ids: string[]): Health {
  let out: Health = "idle";
  for (const id of ids) {
    const h = states[id]?.health;
    if (h) out = worst(out, h);
  }
  return out;
}

export function groupNodeIds(nodes: NodeDef[], group: GroupId): string[] {
  return nodes.filter((n) => n.group === group).map((n) => n.id);
}

/** Layer shown in the scenario picker for the layer the audience is currently zoomed into. */
export function layerForZoom(zoom: string | null): Layer {
  switch (zoom) {
    case "sources":
      return "sources";
    case "platform":
      return "engineering";
    case "consumers":
      return "consumers";
    default:
      return "all";
  }
}

export function scenariosForLayer(list: ScenarioSummary[], layer: Layer): ScenarioSummary[] {
  return layer === "all" ? list : list.filter((s) => s.layer === layer || s.layer === "all");
}

/** Staggered animation delay for the downstream ripple (depth = BFS distance from the injected node). */
export function rippleDelayMs(depth: number | undefined): number {
  return depth === undefined ? 0 : Math.min(depth, 10) * 380;
}

export function isActiveFlow(h: Health): boolean {
  return h === "ok" || h === "warn";
}

export function summarize(state: StudioState): string {
  const { fail, warn } = state.counts;
  if (fail) return `${fail} failing${warn ? `, ${warn} warning` : ""}`;
  if (warn) return `${warn} warning`;
  return "all green";
}
