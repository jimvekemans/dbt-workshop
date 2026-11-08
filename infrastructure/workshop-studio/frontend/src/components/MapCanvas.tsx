import {
  Background,
  BaseEdge,
  Controls,
  Handle,
  Position,
  ReactFlow,
  getBezierPath,
  useReactFlow,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import { memo, useEffect, useMemo } from "react";
import { aggregate, groupNodeIds, isActiveFlow, rippleDelayMs } from "../lib/health";
import type { EdgeDef, GroupDef, GroupId, Health, NodeDef, StudioState } from "../types";

const KIND_ICON: Record<string, string> = {
  source_team: "👥", team: "🧑‍💻", channel: "📡", landing: "🗄", dbt: "◆", git: "⎇", ci: "✔", airflow: "🌀", db: "🛢", report: "📊", consumer: "🏢",
};

interface StudioData extends Record<string, unknown> {
  def: NodeDef;
  health: Health;
  note: string;
  metric: string;
  rippleKey: string;
  rippleDepth: number | undefined;
  selected: boolean;
  dim: boolean;
  glow: boolean;
  hasKnobs: boolean;
  observed: boolean;
}
interface GroupData extends Record<string, unknown> {
  def: GroupDef;
  health: Health;
  active: boolean;
  onZoom: (g: GroupId) => void;
}
interface PacketData extends Record<string, unknown> {
  health: Health;
  kind: EdgeDef["kind"];
  pulse: number;
  dim: boolean;
}

type StudioNode = Node<StudioData, "studio">;
type GroupNodeT = Node<GroupData, "group">;
type PacketEdgeT = Edge<PacketData, "packet">;

const SIDES = { top: Position.Top, bottom: Position.Bottom, left: Position.Left, right: Position.Right } as const;
const ALL_SIDES = ["top", "bottom", "left", "right"] as const;

const StudioNodeView = memo(function StudioNodeView({ data }: NodeProps<StudioNode>) {
  const { def, health } = data;
  const style = data.rippleDepth === undefined ? undefined : ({ "--ripple-delay": `${rippleDelayMs(data.rippleDepth)}ms` } as React.CSSProperties);
  return (
    <div
      key={data.rippleKey}
      className={`snode h-${health} k-${def.kind} o-${def.owner ?? "none"}${data.selected ? " sel" : ""}${data.dim ? " dim" : ""}${data.glow ? " glow" : ""}${data.rippleDepth !== undefined ? " ripple" : ""}`}
      style={{ ...style, width: def.w ?? 176 }}
      title={def.desc}
    >
      {ALL_SIDES.map((s) => (
        <span key={s}>
          <Handle id={`t-${s}`} type="target" position={SIDES[s]} className="hdl" isConnectable={false} />
          <Handle id={`s-${s}`} type="source" position={SIDES[s]} className="hdl" isConnectable={false} />
        </span>
      ))}
      <div className="snode-top">
        <span className="snode-icon" aria-hidden>{KIND_ICON[def.kind] ?? "•"}</span>
        <span className="snode-label">{def.label}</span>
        <span className={`dot d-${health}`} aria-label={health} />
      </div>
      {data.note ? <div className="snode-note">{data.note}</div> : data.metric ? <div className="snode-metric">{data.metric}</div> : null}
      {data.hasKnobs ? <span className="snode-knob" title="has knobs">⚙</span> : null}
      {data.observed ? <span className="snode-obs" title="observed from the live stack">●live</span> : null}
    </div>
  );
});

const GroupNodeView = memo(function GroupNodeView({ data }: NodeProps<GroupNodeT>) {
  const { def } = data;
  return (
    <div className={`gnode g-${def.id} h-${data.health}${data.active ? " active" : ""}`} style={{ width: def.w, height: def.h }}>
      <button className="gnode-head nodrag" onClick={() => data.onZoom(def.id)} title="Zoom into this layer">
        <span className={`dot d-${data.health}`} />
        <span className="gnode-title">{def.label}</span>
        <span className="gnode-sub">{def.subtitle}</span>
        <span className="gnode-zoom">⤢</span>
      </button>
    </div>
  );
});

const PacketEdgeView = memo(function PacketEdgeView(props: EdgeProps<PacketEdgeT>) {
  const { sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, data } = props;
  const [path] = getBezierPath({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition });
  const health = data?.health ?? "idle";
  const active = isActiveFlow(health) && !data?.dim;
  const dur = data?.kind === "data" ? 3.4 : 5.2;
  return (
    <g className={`pedge e-${health} ek-${data?.kind ?? "data"}${data?.dim ? " dim" : ""}`}>
      <BaseEdge path={path} className="pedge-line" />
      {active && (
        <>
          <circle r={3.2} className="packet">
            <animateMotion dur={`${dur}s`} repeatCount="indefinite" path={path} />
          </circle>
          <circle r={3.2} className="packet">
            <animateMotion dur={`${dur}s`} begin={`${dur / 2}s`} repeatCount="indefinite" path={path} />
          </circle>
          <circle key={data?.pulse} r={5} className="packet burst">
            <animateMotion dur="1.1s" repeatCount="1" path={path} />
          </circle>
        </>
      )}
    </g>
  );
});

const nodeTypes = { studio: StudioNodeView, group: GroupNodeView };
const edgeTypes = { packet: PacketEdgeView };

function handlesFor(a: NodeDef, b: NodeDef): { s: string; t: string } {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  if (Math.abs(dy) > Math.abs(dx) * 1.2) return dy > 0 ? { s: "s-bottom", t: "t-top" } : { s: "s-top", t: "t-bottom" };
  return dx >= 0 ? { s: "s-right", t: "t-left" } : { s: "s-left", t: "t-right" };
}

function metricLine(metrics: Record<string, unknown>): string {
  const pick = ["lag_days", "rows", "tag", "rows_landed_last", "watermark", "latest_tag"];
  for (const k of pick) if (metrics[k] !== undefined && metrics[k] !== "" && metrics[k] !== "-") return `${k.replace(/_/g, " ")}: ${metrics[k]}`;
  return "";
}

export interface MapProps {
  groups: GroupDef[];
  nodes: NodeDef[];
  edges: EdgeDef[];
  state: StudioState;
  pulse: number;
  selectedId: string | null;
  highlight: Set<string>;
  ownerFocus: string | null;
  zoom: { target: string | null; nonce: number };
  onSelect: (id: string | null) => void;
  onZoomGroup: (g: GroupId) => void;
}

export function MapCanvas(p: MapProps) {
  const { fitView } = useReactFlow();
  const rippleKey = `${p.state.scenarioId ?? "-"}:${p.state.phase === "injected" || p.state.stepIndex === 0 ? "inj" : "run"}`;

  const rfNodes = useMemo<Array<StudioNode | GroupNodeT>>(() => {
    const out: Array<StudioNode | GroupNodeT> = p.groups.map((g) => ({
      id: `group:${g.id}`,
      type: "group",
      position: { x: g.x, y: g.y },
      data: {
        def: g,
        health: aggregate(p.state.nodes, groupNodeIds(p.nodes, g.id)),
        active: p.zoom.target === g.id,
        onZoom: p.onZoomGroup,
      },
      draggable: false,
      selectable: false,
      zIndex: -1,
      style: { width: g.w, height: g.h },
    }));
    const dimming = p.highlight.size > 0 || p.ownerFocus !== null;
    for (const n of p.nodes) {
      const s = p.state.nodes[n.id];
      const lit = p.highlight.has(n.id) || (p.ownerFocus !== null && n.owner === p.ownerFocus);
      out.push({
        id: n.id,
        type: "studio",
        position: { x: n.x, y: n.y },
        draggable: false,
        data: {
          def: n,
          health: s?.health ?? "idle",
          note: s?.note ?? "",
          metric: metricLine(s?.metrics ?? {}),
          rippleKey,
          rippleDepth: p.state.scenarioId ? p.state.ripple[n.id] : undefined,
          selected: p.selectedId === n.id,
          dim: dimming && !lit,
          glow: lit && dimming,
          hasKnobs: n.knobs.length > 0,
          observed: !!s?.observed && p.state.mode === "live",
        },
      });
    }
    return out;
  }, [p.groups, p.nodes, p.state, p.selectedId, p.highlight, p.ownerFocus, p.zoom.target, p.onZoomGroup, rippleKey]);

  const rfEdges = useMemo<PacketEdgeT[]>(() => {
    const byId = new Map(p.nodes.map((n) => [n.id, n]));
    const dimming = p.highlight.size > 0;
    return p.edges.map((e, i) => {
      const a = byId.get(e.source)!;
      const b = byId.get(e.target)!;
      const h = handlesFor(a, b);
      const sh = p.state.nodes[e.source]?.health ?? "idle";
      const th = p.state.nodes[e.target]?.health ?? "idle";
      const worstHealth: Health = sh === "fail" || th === "fail" ? "fail" : sh === "warn" || th === "warn" ? "warn" : sh === "idle" ? "idle" : "ok";
      return {
        id: `e${i}`,
        source: e.source,
        target: e.target,
        sourceHandle: h.s,
        targetHandle: h.t,
        type: "packet",
        data: { health: worstHealth, kind: e.kind, pulse: p.pulse, dim: dimming && !(p.highlight.has(e.source) && p.highlight.has(e.target)) },
      };
    });
  }, [p.edges, p.nodes, p.state.nodes, p.pulse, p.highlight]);

  const idsByGroup = useMemo(() => {
    const m: Record<string, string[]> = {};
    for (const g of p.groups) m[g.id] = [`group:${g.id}`, ...groupNodeIds(p.nodes, g.id)];
    return m;
  }, [p.groups, p.nodes]);

  useEffect(() => {
    const t = p.zoom.target;
    const ids = t ? (idsByGroup[t] ?? (p.nodes.some((n) => n.id === t) ? [t] : null)) : null;
    const timer = setTimeout(() => {
      if (ids) fitView({ nodes: ids.map((id) => ({ id })), duration: 800, padding: t && idsByGroup[t] ? 0.06 : 0.9 });
      else fitView({ duration: 800, padding: 0.04 });
    }, 30);
    return () => clearTimeout(timer);
  }, [p.zoom.target, p.zoom.nonce, fitView, idsByGroup, p.nodes]);

  return (
    <ReactFlow
      nodes={rfNodes}
      edges={rfEdges}
      nodeTypes={nodeTypes}
      edgeTypes={edgeTypes}
      fitView
      fitViewOptions={{ padding: 0.04 }}
      minZoom={0.12}
      maxZoom={1.9}
      nodesDraggable={false}
      nodesConnectable={false}
      proOptions={{ hideAttribution: true }}
      onNodeClick={(_, n) => p.onSelect(n.type === "studio" ? n.id : null)}
      onPaneClick={() => p.onSelect(null)}
    >
      <Background gap={28} size={1.2} />
      <Controls showInteractive={false} />
    </ReactFlow>
  );
}
