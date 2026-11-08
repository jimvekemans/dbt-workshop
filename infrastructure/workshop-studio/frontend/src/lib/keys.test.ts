import { describe, expect, it } from "vitest";
import { keyAction } from "./keys";

const body = { tagName: "BODY" };

describe("keyAction", () => {
  it("maps the presenter shortcuts in Sim mode", () => {
    expect(keyAction("ArrowRight", body)).toEqual({ type: "next" });
    expect(keyAction("t", body)).toEqual({ type: "tick" });
    expect(keyAction("Escape", body)).toEqual({ type: "overview" });
    expect(keyAction("2", body)).toEqual({ type: "zoom", target: "platform" });
  });

  it("never runs live actions from the keyboard (a clicker sends → too)", () => {
    expect(keyAction("ArrowRight", body, true)).toEqual({ type: "blocked", what: "next" });
    expect(keyAction("T", body, true)).toEqual({ type: "blocked", what: "tick" });
    // navigation stays available in Live mode
    expect(keyAction("Escape", body, true)).toEqual({ type: "overview" });
    expect(keyAction("1", body, true)).toEqual({ type: "zoom", target: "sources" });
  });

  it("ignores keys typed into form fields", () => {
    expect(keyAction("ArrowRight", { tagName: "INPUT" })).toBeNull();
    expect(keyAction("t", { tagName: "TEXTAREA" }, true)).toBeNull();
    expect(keyAction("x", body)).toBeNull();
  });
});
