// Contract with the backend (spec sections 4, 5 and 8). Everything the UI knows about the server is in this file.

export type EnvName = 'dev' | 'val' | 'prd'
export type PartId = 'problem' | 'idea' | 'code' | 'demo'
export type Mode = 'live' | 'replay'
/** `fix` marks the one box that is the safeguard (flow only). Anything unknown is drawn as `idle`. */
export type VisualState = 'ok' | 'bad' | 'warn' | 'idle' | 'fix'

// ---------------------------------------------------------------- catalog
export interface RelatedLink {
  id: string
  why: string
}
export interface CatalogScenario {
  id: string
  title: string
  promise: string
  minutes: number
  environment: EnvName
  related: RelatedLink[]
  recorded: boolean
  /** Optional: the backend may mark the scenario it recommends to start with. */
  first_pick?: boolean
}
export interface CatalogGroup {
  id: string
  title: string
  blurb: string
  scenarios: CatalogScenario[]
}
export interface Catalog {
  groups: CatalogGroup[]
  glossary: Record<string, string>
  /** Where the overview page of the whole stack lives (the backend knows; null/absent on old servers). */
  overview_url?: string | null
  /** Optional: the id of the scenario to recommend first. Without it (and without a marked scenario) the first one in the list is. */
  first_pick?: string | null
}

// ---------------------------------------------------------------- visuals (parts 1 and 2)
export interface FlowNode {
  id: string
  label: string
  state: VisualState
  note?: string | null
}
export interface FlowVisual {
  type: 'flow'
  nodes: FlowNode[]
  links: [string, string][]
  caption?: string | null
}
export interface TimelineDay {
  label: string
  state: VisualState
  note?: string | null
}
export interface TimelineVisual {
  type: 'timeline'
  days: TimelineDay[]
  marker?: { after: number; label: string }
  caption?: string | null
}
export interface CompareSide {
  title: string
  items: string[]
  state: VisualState
}
export interface CompareVisual {
  type: 'compare'
  left: CompareSide
  right: CompareSide
  caption?: string | null
}
export type Visual = FlowVisual | TimelineVisual | CompareVisual

// ---------------------------------------------------------------- scenario
export interface CodeLine {
  n: number
  text: string
  note?: string | null
}
export interface CodeBlockData {
  title: string
  /** A plain description of the file ("The recipe for the table of parcel scans"): the heading of the block. */
  file_label: string
  /** Path of the file inside the project (may start with ../ for files outside it). */
  file: string
  /** sql | jinja-sql | yaml | python | text (anything else is shown unhighlighted) */
  language: string
  /** Empty for files that are not in the repository: then no link is shown. */
  gitlab_url?: string | null
  lines: CodeLine[]
}

export interface ChoiceOption {
  value: string
  label: string
}
export type InputType = 'day' | 'number' | 'choice' | 'toggle'
/**
 * One input exactly as the backend sends it: every optional key is present and null when it does not apply
 * (a day has min/max, a number has min/max/step/unit, a choice has options). `default` is already rendered.
 */
export interface StepInput {
  id: string
  type: InputType
  label: string
  /** A toggle may say something different for each state. */
  help?: string | { on: string; off: string } | null
  /** A `day` input may name one day on the track (for example the real change day). */
  mark?: { day: string; label: string } | null
  default: string | number | boolean
  min?: string | number | null
  max?: string | number | null
  step?: number | null
  unit?: string | null
  options?: ChoiceOption[] | null
}
export type InputValue = string | number | boolean
export type InputValues = Record<string, InputValue>

export type EvidenceType =
  | 'table'
  | 'bars'
  | 'number'
  | 'tasks'
  | 'pipeline'
  | 'mail'
  | 'versions'
  | 'result'
  | 'freshness'
  | 'run_history'

/**
 * A `show:` item. The backend sends the whole definition including keys the UI never needs (query, dag, for, match,
 * env, as, columns, highlight, compare_to, limit ...); everything shown comes from `data`.
 */
export interface EvidenceItem {
  type: EvidenceType
  title?: string | null
  caption?: string | null
  /** Show this step's final data of the same item type as a small "Before" above the new result. */
  before_of?: string | null
  /** Recorded (replay) frames: when the recording was made. */
  recorded_at?: string | null
  [extra: string]: unknown
}

export interface StepDef {
  id: string
  title: string
  say?: string | null
  button: string
  inputs: StepInput[]
  show: EvidenceItem[]
  outcome?: string | null
  /** How long this step usually takes (from its recording); null when unknown. */
  usual_seconds?: number | null
}

export interface Scenario {
  id: string
  group: string
  title: string
  promise: string
  minutes: number
  environment: EnvName
  related: RelatedLink[]
  problem: { headline: string; story: string; cost: string; visual: Visual }
  idea: { headline: string; points: string[]; visual: Visual }
  code: CodeBlockData[]
  demo: { intro: string; steps: StepDef[] }
  /** How long getting this demo ready usually takes; null when unknown. */
  prepare_usual_seconds?: number | null
}

// ---------------------------------------------------------------- evidence data (the `data` key of a live evidence item)
/** The state words of tasks, pipeline jobs and the like. */
export type RunState = 'todo' | 'running' | 'done' | 'failed' | 'skipped'
export type Cell = string | number | boolean | null

export interface TableData {
  /** `unit` is shown in the header ("Time (seconds)"): a number without its unit is never left to the reader. */
  columns: { key: string; label: string; unit?: string | null }[]
  /** `details`: the technical name behind a cell that now shows plain words; only shown under "Technical details". */
  rows: { cells: Record<string, Cell>; state: 'good' | 'bad' | null; details?: string }[]
  empty: string
  truncated: boolean
  /** One line under the table ("Showing the 9 slowest of 28 tables"). The item may carry it too. */
  footer?: string | null
}
export interface BarsData {
  labels: (string | number)[]
  /** Optional, oldest first and today last: when present it is drawn instead of `labels`/`series` (which stay for old data). */
  bars?: { key: string; label: string; today?: boolean }[]
  x_label: string
  /** What one bar counts ("parcel scans"), written above the y axis. Not sent by the backend yet. */
  unit?: string
  series: { name: string; state: 'good' | 'bad' | 'neutral'; values: number[] }[]
}
export interface NumberData {
  value: number | null
  unit: string
  compare: { label: string; value: number | null } | null
}
export interface TasksData {
  title: string
  run?: { state: RunState; state_text: string; started: string | null; finished: string | null } | null
  /** `count`: several identical bookkeeping tasks collapsed into one row. `state_text` may be a plain sentence ("Done in the first attempt"). */
  tasks: {
    label: string
    state: RunState
    state_text: string
    count?: number
    /** A step that was skipped on purpose in a run without a failure (clean-up, failure mail): shown as "Not needed". */
    not_needed?: boolean
    /** Repeated steps ("Loading one part of the new copy") are numbered "Part 1 of 4". */
    part?: number
    parts?: number
  }[]
  empty?: string
}
export interface PipelineFailure {
  kind: 'check' | 'table' | string
  text: string
  rows?: number | null
  details?: string | null
}
export interface PipelineData {
  state: RunState
  state_text: string
  /** In the order they run. `manual` is a job that waits for a person. */
  jobs: { label: string; state: RunState | 'manual'; state_text: string }[]
  failures?: PipelineFailure[]
  /** What the rehearsal covered: "1 of 28 tables rehearsed", then the checks. */
  rehearsed?: { tables: number; checks?: number | null; tables_total?: number | null; /** build time in seconds */ seconds?: number | null; /** plain names of the rehearsed tables; the last may read "and 22 more" */ names?: string[] | null } | null
}
export type MailData =
  | { found: false; empty: string }
  | { found: true; subject: string; from: string; received: string | null; html: string }
export interface VersionsData {
  environments: { env: EnvName; label: string; tag: string | null }[]
}
/** One line of a two-list comparison (`reasons` of a result): the tool, what each list has, and whether they agree. */
export interface ResultReason {
  label: string
  left?: string | null
  right?: string | null
  same?: boolean | null
}
export interface ResultData {
  summary: string
  /** Free-form facts of the last action (refused, reasons, run_id ...). */
  data: Record<string, unknown>
  /** Technical text: only shown under "Technical details (for engineers)". */
  details?: string | null
}
export type FreshnessState = 'fresh' | 'late' | 'very_late' | 'unknown'
export interface FreshnessData {
  generated_at?: string | null
  sources: { label: string; state: FreshnessState; state_text: string; last_data: string }[]
  empty?: string
}
export interface RunHistoryData {
  runs: {
    started: string
    /** `warn`: no failure, but a warning ("1 warning", amber). */
    state: 'good' | 'warn' | 'bad'
    summary: string
    current?: boolean
    /** True for a run that resumed an earlier one: labelled "Retry". */
    retry?: boolean
    tables_built?: number | null
    /** Tables in this run. */
    tables_total?: number | null
    tables_not_run?: number | null
    tables_failed?: number | null
    /** Tables of the project that were left out of this run on purpose. */
    tables_left_out?: number | null
    checks_passed?: number | null
    checks_failed?: number | null
    checks_warned?: number | null
    checks_not_run?: number | null
    /** Older recordings only: */
    not_run?: number | null
    checks_total?: number | null
  }[]
  empty: string
}
/** What a provider that failed leaves behind: a plain sentence. */
export interface ErrorData {
  error: string
}
export type EvidenceData = TableData | BarsData | NumberData | TasksData | PipelineData | MailData | VersionsData | ResultData | FreshnessData | RunHistoryData

/** An evidence item as it comes in the live state: the item plus its data (null until the first read). */
export type LiveEvidence = EvidenceItem & { data?: EvidenceData | ErrorData | null }

// ---------------------------------------------------------------- live state
export type StackStatus = 'up' | 'down' | 'starting'
export interface StackService {
  id: string
  label: string
  status: StackStatus
  detail?: string
}
export type BaselineStatus = 'ready' | 'preparing' | 'missing' | 'failed'
export type ProgressStatus = 'todo' | 'running' | 'done' | 'failed'
export interface Baseline {
  status: BaselineStatus
  /** A plain sentence: what is being checked, or why it failed. */
  detail: string
  progress: { label: string; status: ProgressStatus }[]
}
export interface Feeder {
  running: boolean
  simulated_day: string | null
  /** Per environment (dev, val, prd). */
  simulated_days?: Record<string, string | null>
  /** ISO timestamp without a zone. */
  last_delivery: string | null
  rows?: Record<string, Record<string, number>>
}
export type JobKind = 'prepare' | 'step' | 'cleanup'
export interface ActionResult {
  label: string
  status: ProgressStatus
  summary?: string
  details?: string
}
export interface Job {
  kind: JobKind
  /** null while the whole stack is checked (prepare without a scenario). */
  scenario: string | null
  step: string | null
  label: string
  /** Epoch SECONDS (a float), not an ISO string. */
  started: number
  cancellable: boolean
  /** What the job has done so far (the setup actions while a demo is made ready). */
  actions?: ActionResult[]
}
export interface Leftover {
  label: string
  from_scenario: string | null
}
export type StepStatus = 'todo' | 'running' | 'done' | 'failed' | 'stopped'
export interface DemoStep {
  id: string
  status: StepStatus
  summary?: string | null
  outcome?: string | null
  /** `not_quite` when the step ran but the result still shows the problem. Absent means good. */
  outcome_kind?: 'good' | 'not_quite'
  actions: ActionResult[]
  evidence: LiveEvidence[]
}
/** There is no 'closed': after a clean up `demo` is null. */
export type DemoStatus = 'preparing' | 'ready' | 'running' | 'failed'
export interface Demo {
  scenario: string
  status: DemoStatus
  /** A plain sentence about why the demo failed to open. Empty otherwise. */
  message?: string
  steps: DemoStep[]
}
export interface Notice {
  level: 'info' | 'problem'
  text: string
}
export interface ServerState {
  mode: Mode
  /** A plain sentence when Live cannot be used (the stack is not reachable); the server then stays in Replay. */
  live_unavailable: string | null
  stack: StackService[]
  baseline: Baseline
  feeder: Feeder
  job: Job | null
  leftovers: Leftover[]
  /** What the last job wants the presenter to know. Cleared when the next job starts. */
  notice: Notice | null
  /** Set while a prepare or clean-up waits for the baseline check: a calm waiting line, not an error. */
  waiting?: { label: string; text: string } | null
  demo: Demo | null
}

// ---------------------------------------------------------------- transport
export class ApiError extends Error {
  status: number
  constructor(message: string, status: number) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }
}

export interface Subscription {
  onState: (s: ServerState) => void
  onConnection: (connected: boolean) => void
}

/** What the app needs from a backend: the real HTTP one or the in-browser mock. */
export interface Backend {
  getCatalog(): Promise<Catalog>
  getScenario(id: string): Promise<Scenario>
  getState(): Promise<ServerState>
  prepare(id: string): Promise<ServerState>
  runStep(id: string, step: string, inputs: InputValues): Promise<ServerState>
  cleanup(): Promise<ServerState>
  cancel(): Promise<ServerState>
  setMode(mode: Mode): Promise<ServerState>
  setFeeder(running: boolean): Promise<ServerState>
  subscribe(sub: Subscription): () => void
}
