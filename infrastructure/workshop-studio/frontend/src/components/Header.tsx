import { api } from "../api";
import { summarize } from "../lib/health";
import type { Studio } from "../store";
import type { StudioState } from "../types";

const PILLS: Array<[string, string]> = [
  ["singlestore", "SingleStore"],
  ["gitlab", "GitLab"],
  ["airflow-dev", "airflow-dev"],
  ["airflow-prod", "airflow-prod"],
];

export const LIVE_RESET_WARNING =
  "Reset the LIVE stack?\n\n" +
  "- SingleStore: landing tables are truncated and reloaded, all channel faults are cleared.\n" +
  "- GitLab: dev and main are force-pushed back to the baseline, and EVERY other branch and tag is deleted - " +
  "including participants' own feature branches.\n\n" +
  "This cannot be undone.";

export function Header({ studio, state, large, onLarge }: { studio: Studio; state: StudioState; large: boolean; onLarge: () => void }) {
  const live = state.mode === "live";
  const onReset = () => {
    // Sim (or dry-run) reset only rewinds the map; a live one is destructive and needs a deliberate yes
    if (live && !state.dryRun && !window.confirm(LIVE_RESET_WARNING)) return;
    studio.run(api.reset);
  };
  return (
    <header className="hdr">
      <div className="brand">
        <span className="logo" aria-hidden>◈</span>
        <div>
          <h1>Workshop Studio</h1>
          <span className="tag">struggles of a dbt data platform</span>
        </div>
      </div>

      <div className="seg" role="group" aria-label="Execution mode">
        <button className={!live ? "on" : ""} onClick={() => studio.run(() => api.setMode("sim"))} title="Pure simulation - nothing touches the stack">Sim</button>
        <button className={live ? "on live" : ""} onClick={() => studio.run(() => api.setMode("live"))} title="Run every action against SingleStore / GitLab / Airflow">Live</button>
      </div>
      {live && (
        <label className="chk" title="Render every command in the log but execute nothing">
          <input type="checkbox" checked={state.dryRun} onChange={(e) => studio.run(() => api.setDryRun(e.target.checked))} /> dry-run
        </label>
      )}

      <div className="pills">
        {live &&
          PILLS.map(([key, label]) => {
            const p = state.probes[key];
            const ok = typeof p === "object" && p?.ok;
            const known = typeof p === "object" && p && "ok" in p;
            return (
              <span key={key} className={`pill ${!known ? "p-unk" : ok ? "p-ok" : "p-bad"}`} title={typeof p === "object" && p?.error ? p.error : label}>
                {label}
              </span>
            );
          })}
        {!live && <span className="pill p-sim">offline simulation</span>}
      </div>

      <div className={`overall o-${state.counts.fail ? "fail" : state.counts.warn ? "warn" : "ok"}`} role="status">{summarize(state)}</div>
      <div className="spacer" />
      {state.busy && <span className="busy" title={state.busy}><i className="spin" /> {state.busy}</span>}
      {state.busy && live && (
        <button className="btn ghost sm" onClick={() => studio.run(api.cancel)} title="Stop at the next wait point (CI, tag, Airflow poll). Finished live actions stay done.">
          ✕ Cancel
        </button>
      )}
      <span className={`conn ${studio.connected ? "c-on" : "c-off"}`} title={studio.connected ? "websocket connected" : "reconnecting..."}>●</span>
      <button className="btn ghost" onClick={onLarge} aria-pressed={large} title="Projector mode: larger text">{large ? "A−" : "A+"}</button>
      {live && state.liveFaultsPending?.length > 0 && (
        <span className="pill p-bad" title="Channel faults injected live are still set in SingleStore. Clear the scenario (in Live mode) or Reset to undo them.">
          faults set: {state.liveFaultsPending.join(", ")}
        </span>
      )}
      <button className="btn ghost" onClick={onReset} title="Back to sprint day 0 (Live: also resets landing data, channels and GitLab branches)">Reset</button>
    </header>
  );
}
