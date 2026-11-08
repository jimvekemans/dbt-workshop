import type { LogEntry, StudioState, WsEvent } from "../types";

export interface Model {
  state: StudioState | null;
  log: LogEntry[];
  /** bumps on every inject / tick so edges can fire a one-shot packet burst */
  pulse: number;
}

export const MAX_LOG = 400;

export const initialModel: Model = { state: null, log: [], pulse: 0 };

export function applyEvent(model: Model, ev: WsEvent): Model {
  if (ev.type === "log") {
    const log = [...model.log, ev.entry];
    return { ...model, log: log.length > MAX_LOG ? log.slice(log.length - MAX_LOG) : log };
  }
  const prev = model.state;
  const changed =
    !prev || prev.sprintDay !== ev.state.sprintDay || prev.scenarioId !== ev.state.scenarioId || prev.stepIndex !== ev.state.stepIndex;
  return { ...model, state: ev.state, pulse: changed ? model.pulse + 1 : model.pulse };
}

export function seed(state: StudioState, log: LogEntry[]): Model {
  return { state, log: log.slice(-MAX_LOG), pulse: 0 };
}
