import { describe, expect, it } from "vitest";
import { applyEvent, initialModel, MAX_LOG, seed } from "./reducer";
import { keyAction } from "./keys";
import type { StudioState } from "../types";

const st = (over: Partial<StudioState> = {}): StudioState =>
  ({ mode: "sim", sprintDay: 0, scenarioId: null, stepIndex: 0, nodes: {}, counts: { ok: 0, warn: 0, fail: 0, idle: 0 }, ...over }) as StudioState;

describe("state reducer", () => {
  it("replaces state and pulses only when the story moves", () => {
    let m = applyEvent(initialModel, { type: "state", state: st() });
    expect(m.state?.sprintDay).toBe(0);
    const p0 = m.pulse;
    m = applyEvent(m, { type: "state", state: st({ mode: "live" }) });      // mode flip: no pulse
    expect(m.pulse).toBe(p0);
    m = applyEvent(m, { type: "state", state: st({ sprintDay: 1 }) });      // tick: pulse
    expect(m.pulse).toBe(p0 + 1);
    m = applyEvent(m, { type: "state", state: st({ sprintDay: 1, scenarioId: "S1" }) });
    expect(m.pulse).toBe(p0 + 2);
  });

  it("caps the log", () => {
    let m = seed(st(), []);
    for (let i = 0; i < MAX_LOG + 25; i++) m = applyEvent(m, { type: "log", entry: { t: "00:00:00", level: "info", msg: String(i) } });
    expect(m.log).toHaveLength(MAX_LOG);
    expect(m.log[m.log.length - 1].msg).toBe(String(MAX_LOG + 24));
  });
});

describe("presenter keyboard shortcuts", () => {
  it("maps keys", () => {
    expect(keyAction("ArrowRight", null)).toEqual({ type: "next" });
    expect(keyAction("Escape", null)).toEqual({ type: "overview" });
    expect(keyAction("1", null)).toEqual({ type: "zoom", target: "sources" });
    expect(keyAction("2", null)).toEqual({ type: "zoom", target: "platform" });
    expect(keyAction("3", null)).toEqual({ type: "zoom", target: "consumers" });
    expect(keyAction("t", null)).toEqual({ type: "tick" });
    expect(keyAction("x", null)).toBeNull();
  });

  it("ignores typing in form fields", () => {
    expect(keyAction("1", { tagName: "INPUT" })).toBeNull();
    expect(keyAction("ArrowRight", { tagName: "SELECT" })).toBeNull();
    expect(keyAction("t", { tagName: "DIV", isContentEditable: true })).toBeNull();
  });
});
