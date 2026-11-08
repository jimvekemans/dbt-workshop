import type { ScenarioDetail, StudioState, World } from "./types";

// Every request carries X-Studio: the backend refuses state-changing requests without it. Other web pages open in
// the same browser cannot add a custom header without a CORS preflight, which the studio never grants.
const HEADERS = { "X-Studio": "1" };

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? HEADERS : { ...HEADERS, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let detail = res.statusText;
    try {
      detail = (await res.json()).detail ?? detail;
    } catch {
      /* not json */
    }
    throw new Error(String(detail));
  }
  return (await res.json()) as T;
}

export const api = {
  world: () => call<World>("GET", "/world"),
  scenario: (id: string) => call<ScenarioDetail>("GET", `/scenarios/${id}`),
  setMode: (mode: "sim" | "live") => call<StudioState>("POST", "/mode", { mode }),
  setDryRun: (dry_run: boolean) => call<StudioState>("POST", "/dry-run", { dry_run }),
  knob: (node: string, key: string, value: unknown) => call<StudioState>("POST", "/knob", { node, key, value }),
  tick: () => call<StudioState>("POST", "/tick"),
  reset: () => call<StudioState>("POST", "/reset"),
  inject: (id: string) => call<StudioState>("POST", `/scenarios/${id}/inject`),
  nextStep: () => call<StudioState>("POST", "/step/next"),
  clearScenario: () => call<StudioState>("POST", "/scenario/clear"),
  probe: () => call<StudioState>("POST", "/probe"),
  cancel: () => call<StudioState>("POST", "/cancel"),
};
