export type Health = "ok" | "warn" | "fail" | "idle";
export type GroupId = "sources" | "platform" | "consumers";
export type Layer = "all" | "sources" | "engineering" | "consumers";
export type Mode = "sim" | "live";

export interface KnobDef {
  key: string;
  label: string;
  type: "bool" | "int" | "select";
  default: boolean | number | string;
  min?: number | null;
  max?: number | null;
  options?: string[] | null;
}

export interface NodeDef {
  id: string;
  label: string;
  kind: string;
  group: GroupId;
  x: number;
  y: number;
  w?: number | null;
  h?: number | null;
  desc: string;
  owner?: string | null;
  knobs: KnobDef[];
  metrics: Record<string, unknown>;
}

export interface EdgeDef {
  source: string;
  target: string;
  kind: "data" | "code" | "ci";
}

export interface GroupDef {
  id: GroupId;
  label: string;
  subtitle: string;
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface SprintDay {
  day: number;
  title: string;
  milestone?: string | null;
  /** Live mode: what this day only animates (its live part is just the trickle) */
  live_note?: string | null;
}

export interface ScenarioSummary {
  id: string;
  layer: Layer;
  title: string;
  summary: string;
  star: boolean;
  status: "implemented" | "narrative";
  focus: string[];
  detect: string;
  stepCount: number;
  live: boolean;
  /** Live-mode preconditions, as the hint the presenter reads ("press Reset first") */
  requires: string[];
}

export interface NodeState {
  health: Health;
  simHealth: Health;
  note: string;
  metrics: Record<string, unknown>;
  knobs: Record<string, unknown>;
  observed: boolean;
}

export interface StudioState {
  mode: Mode;
  dryRun: boolean;
  sprintDay: number;
  sprintLength: number;
  scenarioId: string | null;
  phase: "idle" | "injected" | "resolving" | "resolved";
  stepIndex: number;
  stepCount: number;
  nodes: Record<string, NodeState>;
  ripple: Record<string, number>;
  counts: Record<Health, number>;
  busy: string | null;
  error: string | null;
  probes: Record<string, { ok?: boolean; error?: string } | string>;
  touched: string[];
  /** bumps when the scenario YAML was reloaded: the UI refetches the world */
  contentVersion: number;
  /** channel faults a live injection left in the stack ("kafka@prod"); Clear or Reset undoes them */
  liveFaultsPending: string[];
}

export interface LogEntry {
  t: string;
  level: "info" | "warn" | "error" | "ok";
  msg: string;
}

export interface World {
  groups: GroupDef[];
  nodes: NodeDef[];
  edges: EdgeDef[];
  sprint: { days_step: number; days: SprintDay[] };
  scenarios: ScenarioSummary[];
  state: StudioState;
  log: LogEntry[];
  /** why the last YAML edit was not loaded (the previous content stays active), or null */
  contentError: string | null;
}

export interface StepDetail {
  title: string;
  explain: string;
  code: string;
  zoom?: string | null;
  highlight: string[];
  rendered: string[];
  hasLive: boolean;
  expect?: { node: string; health: Health } | null;
}

export interface ScenarioDetail extends Omit<ScenarioSummary, "stepCount" | "live"> {
  inject: { narrative: string };
  injectRendered: string[];
  steps: StepDetail[];
}

export type WsEvent = { type: "state"; state: StudioState } | { type: "log"; entry: LogEntry };
