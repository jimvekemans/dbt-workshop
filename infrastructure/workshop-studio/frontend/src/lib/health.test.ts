import { describe, expect, it } from "vitest";
import { aggregate, layerForZoom, rippleDelayMs, scenariosForLayer, summarize, worst } from "./health";
import type { NodeState, ScenarioSummary, StudioState } from "../types";

const ns = (health: NodeState["health"]): NodeState => ({ health, simHealth: health, note: "", metrics: {}, knobs: {}, observed: false });

describe("health propagation helpers", () => {
  it("ranks fail > warn > ok > idle", () => {
    expect(worst("ok", "warn")).toBe("warn");
    expect(worst("fail", "warn")).toBe("fail");
    expect(worst("idle", "ok")).toBe("ok");
    expect(worst("idle", "idle")).toBe("idle");
  });

  it("aggregates a layer by its worst node (what the group header shows when zoomed out)", () => {
    const states = { a: ns("ok"), b: ns("warn"), c: ns("fail"), d: ns("idle") };
    expect(aggregate(states, ["a", "b"])).toBe("warn");
    expect(aggregate(states, ["a", "b", "c"])).toBe("fail");
    expect(aggregate(states, ["d"])).toBe("idle");
    expect(aggregate(states, [])).toBe("idle");
  });

  it("staggers the ripple by BFS depth and caps it", () => {
    expect(rippleDelayMs(undefined)).toBe(0);
    expect(rippleDelayMs(0)).toBe(0);
    expect(rippleDelayMs(3)).toBeGreaterThan(rippleDelayMs(1));
    expect(rippleDelayMs(99)).toBe(rippleDelayMs(10));
  });

  it("maps the zoomed layer to the scenario filter", () => {
    expect(layerForZoom("sources")).toBe("sources");
    expect(layerForZoom("platform")).toBe("engineering");
    expect(layerForZoom("consumers")).toBe("consumers");
    expect(layerForZoom(null)).toBe("all");
    const list = [
      { id: "H0", layer: "all" },
      { id: "S1", layer: "sources" },
      { id: "E1", layer: "engineering" },
    ] as ScenarioSummary[];
    expect(scenariosForLayer(list, "sources").map((s) => s.id)).toEqual(["H0", "S1"]);
    expect(scenariosForLayer(list, "all")).toHaveLength(3);
  });

  it("summarizes counts for the header", () => {
    const base = { counts: { ok: 30, warn: 0, fail: 0, idle: 0 } } as StudioState;
    expect(summarize(base)).toBe("all green");
    expect(summarize({ counts: { ok: 1, warn: 2, fail: 0, idle: 0 } } as StudioState)).toBe("2 warning");
    expect(summarize({ counts: { ok: 1, warn: 2, fail: 3, idle: 0 } } as StudioState)).toBe("3 failing, 2 warning");
  });
});
