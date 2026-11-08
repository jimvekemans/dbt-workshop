export type KeyAction =
  | { type: "next" }
  | { type: "overview" }
  | { type: "zoom"; target: "sources" | "platform" | "consumers" }
  | { type: "tick" }
  /** a shortcut that would run live actions: refused, the presenter has to click */
  | { type: "blocked"; what: "next" | "tick" }
  | null;

const ZOOM = { "1": "sources", "2": "platform", "3": "consumers" } as const;

/** Presenter shortcuts: → next step · Esc zoom out · 1/2/3 zoom to layer · T next sprint day.
 * `liveExec` (Live mode without dry-run): → and T would run real GitLab / Airflow / SingleStore actions, and a
 * presenter clicker sends → too - so they are refused there and only a deliberate click runs a live step. */
export function keyAction(
  key: string,
  target: { tagName?: string; isContentEditable?: boolean } | null,
  liveExec = false,
): KeyAction {
  const tag = (target?.tagName ?? "").toLowerCase();
  if (tag === "input" || tag === "textarea" || tag === "select" || target?.isContentEditable) return null;
  if (key === "ArrowRight") return liveExec ? { type: "blocked", what: "next" } : { type: "next" };
  if (key === "Escape") return { type: "overview" };
  if (key in ZOOM) return { type: "zoom", target: ZOOM[key as keyof typeof ZOOM] };
  if (key === "t" || key === "T") return liveExec ? { type: "blocked", what: "tick" } : { type: "tick" };
  return null;
}
