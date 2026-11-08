import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import { layerForZoom, scenariosForLayer } from "../lib/health";
import type { Studio } from "../store";
import type { Layer, LogEntry, NodeDef, ScenarioDetail, StudioState, World } from "../types";

export type Tab = "scenario" | "playbook" | "code" | "node" | "log";

const LAYERS: Array<[Layer, string]> = [["all", "All"], ["sources", "Sources"], ["engineering", "Platform"], ["consumers", "Consumers"]];

export interface DrawerProps {
  studio: Studio;
  world: World;
  state: StudioState;
  log: LogEntry[];
  tab: Tab;
  setTab: (t: Tab) => void;
  zoomTarget: string | null;
  selected: NodeDef | null;
  detail: ScenarioDetail | null;
  previewStep: number | null;
  setPreviewStep: (n: number | null) => void;
  ownerFocus: string | null;
  setOwnerFocus: (o: string | null) => void;
  runStep: () => void;
}

export function Drawer(p: DrawerProps) {
  const tabs: Array<[Tab, string]> = [["scenario", "Scenario"], ["playbook", "Playbook"], ["code", "Code"], ["node", p.selected ? p.selected.label : "Knobs"], ["log", "Log"]];
  return (
    <aside className="drawer">
      <nav className="tabs" role="tablist">
        {tabs.map(([id, label]) => (
          <button key={id} role="tab" aria-selected={p.tab === id} className={p.tab === id ? "on" : ""} onClick={() => p.setTab(id)}>
            {label}
            {id === "playbook" && p.state.scenarioId && p.state.stepCount > 0 && <em>{p.state.stepIndex}/{p.state.stepCount}</em>}
          </button>
        ))}
      </nav>
      <div className="tabbody">
        {p.state.error && <div className="err" role="alert">{p.state.error}</div>}
        {p.tab === "scenario" && <ScenarioTab {...p} />}
        {p.tab === "playbook" && <PlaybookTab {...p} />}
        {p.tab === "code" && <CodeTab {...p} />}
        {p.tab === "node" && <NodeTab {...p} />}
        {p.tab === "log" && <LogTab log={p.log} />}
      </div>
    </aside>
  );
}

function ScenarioTab({ studio, world, state, zoomTarget, setTab }: DrawerProps) {
  const [layer, setLayer] = useState<Layer>("all");
  useEffect(() => setLayer(layerForZoom(zoomTarget)), [zoomTarget]);
  const list = scenariosForLayer(world.scenarios, layer);
  return (
    <div>
      <div className="chips" role="group" aria-label="Filter by layer">
        {LAYERS.map(([id, label]) => (
          <button key={id} className={layer === id ? "on" : ""} onClick={() => setLayer(id)}>{label}</button>
        ))}
      </div>
      <ul className="cards">
        {list.map((s) => {
          const active = state.scenarioId === s.id;
          return (
            <li key={s.id} className={`card${active ? " active" : ""}`}>
              <div className="card-top">
                <code>{s.id.split("_")[0]}</code>
                <strong>{s.title}</strong>
                {s.star && <span className="badge star" title="Fully implemented, runs live">★</span>}
                {s.status === "narrative" && <span className="badge" title="Narrative + simulation only (Phase 2 for live)">narrative</span>}
              </div>
              <p>{s.summary}</p>
              {s.detect && <p className="detect"><b>Detect:</b> {s.detect}</p>}
              {state.mode === "live" && s.requires?.length > 0 && (
                <p className="detect"><b>Live needs:</b> {s.requires.join(" · ")}</p>
              )}
              <div className="card-act">
                <button
                  className="btn primary"
                  disabled={!!state.busy}
                  onClick={async () => {
                    await studio.run(() => api.inject(s.id));
                    setTab("playbook");
                  }}
                >
                  {s.id.startsWith("H0") ? "▶ Start" : active ? "↻ Re-inject" : "⚡ Inject"}
                </button>
                {state.mode === "live" && !s.live && <span className="hint">no live actions - simulation only</span>}
              </div>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function PlaybookTab({ studio, state, detail, setTab, previewStep, setPreviewStep, runStep }: DrawerProps) {
  if (!state.scenarioId || !detail) return <p className="empty">Inject a scenario first (Scenario tab).</p>;
  const next = state.stepIndex;
  const shown = previewStep ?? Math.min(next, detail.steps.length - 1);
  const resolved = state.phase === "resolved";
  return (
    <div>
      <h2 className="sc-title">{detail.title}</h2>
      <p className="narr">{detail.inject.narrative}</p>
      <ol className="steps">
        {detail.steps.map((s, i) => {
          const done = i < next;
          const cur = i === next && !resolved;
          return (
            <li key={i} className={`step${done ? " done" : ""}${cur ? " cur" : ""}${i === shown ? " shown" : ""}`}>
              <button className="step-head" onClick={() => setPreviewStep(i === previewStep ? null : i)}>
                <span className="step-n">{done ? "✓" : i + 1}</span>
                <span>{s.title}</span>
                {s.hasLive && <span className="badge live">live</span>}
              </button>
              {i === shown && (
                <div className="step-body">
                  <p>{s.explain}</p>
                  {s.code && <button className="linkbtn" onClick={() => setTab("code")}>show code ›</button>}
                  {s.expect && <p className="expect">Expect: <code>{s.expect.node}</code> → <b>{s.expect.health}</b></p>}
                </div>
              )}
            </li>
          );
        })}
      </ol>
      <div className="play-act">
        <button className="btn primary" disabled={resolved || !!state.busy} onClick={runStep}>
          {resolved ? "Resolved ✓" : state.mode === "live" ? "▶ Run step (live)" : "▶ Run step (→)"}
        </button>
        <button className="btn ghost" disabled={!!state.busy} onClick={() => studio.run(api.clearScenario)}>Clear scenario</button>
      </div>
    </div>
  );
}

function Copy({ text }: { text: string }) {
  const [done, setDone] = useState(false);
  return (
    <button className="btn ghost sm" onClick={() => { navigator.clipboard?.writeText(text); setDone(true); setTimeout(() => setDone(false), 1200); }}>
      {done ? "copied" : "copy"}
    </button>
  );
}

function CodeTab({ state, detail, previewStep }: DrawerProps) {
  if (!state.scenarioId || !detail) return <p className="empty">Inject a scenario to see the exact commands that run.</p>;
  const idx = previewStep ?? Math.min(state.stepIndex, detail.steps.length - 1);
  const step = detail.steps[idx];
  const blocks: Array<[string, string]> = [];
  if (step?.code) blocks.push(["What you would type", step.code]);
  if (step?.rendered.length) blocks.push(["What Live mode executes", step.rendered.join("\n")]);
  if (state.stepIndex === 0 && detail.injectRendered.length) blocks.push(["Injection (Live)", detail.injectRendered.join("\n")]);
  return (
    <div>
      <h2 className="sc-title">Step {idx + 1}: {step?.title}</h2>
      {blocks.length === 0 && <p className="empty">This step is a conversation, not a command.</p>}
      {blocks.map(([title, body]) => (
        <section key={title} className="codeblk">
          <header><span>{title}</span><Copy text={body} /></header>
          <pre>{body}</pre>
        </section>
      ))}
    </div>
  );
}

function NodeTab({ studio, state, selected, setOwnerFocus, ownerFocus }: DrawerProps) {
  const ns = selected ? state.nodes[selected.id] : null;
  if (!selected || !ns) return <p className="empty">Click a node on the map. Channels have knobs that inject faults (Live: they write <code>_source_channel_config</code>).</p>;
  const metrics = Object.entries(ns.metrics).filter(([, v]) => v !== "" && v !== undefined);
  return (
    <div>
      <h2 className="sc-title"><span className={`dot d-${ns.health}`} /> {selected.label}</h2>
      <p className="muted">{selected.kind} · {selected.id}</p>
      <p>{selected.desc}</p>
      {ns.note && <p className="note"><b>{ns.health}:</b> {ns.note}</p>}
      {selected.owner && (
        <button className={`chip-btn${ownerFocus === selected.owner ? " on" : ""}`} onClick={() => setOwnerFocus(ownerFocus === selected.owner ? null : selected.owner!)}>
          highlight team “{selected.owner}”
        </button>
      )}
      {metrics.length > 0 && (
        <dl className="metrics">
          {metrics.map(([k, v]) => (<div key={k}><dt>{k.replace(/_/g, " ")}</dt><dd>{String(v)}</dd></div>))}
        </dl>
      )}
      {selected.knobs.length > 0 && (
        <form className="knobs" onSubmit={(e) => e.preventDefault()}>
          <h3>Knobs {state.mode === "live" ? <span className="badge live">writes to SingleStore (prod)</span> : <span className="badge">simulated</span>}</h3>
          {selected.knobs.map((k) => {
            const val = ns.knobs[k.key];
            const set = (v: unknown) => studio.run(() => api.knob(selected.id, k.key, v));
            return (
              <label key={k.key} className="knob">
                <span>{k.label}</span>
                {k.type === "bool" && <input type="checkbox" checked={!!val} onChange={(e) => set(e.target.checked)} />}
                {k.type === "int" && <RangeKnob min={k.min ?? 0} max={k.max ?? 100} value={Number(val ?? 0)} onCommit={set} />}
                {k.type === "select" && (
                  <select value={String(val)} onChange={(e) => set(e.target.value)}>
                    {k.options?.map((o) => <option key={o}>{o}</option>)}
                  </select>
                )}
              </label>
            );
          })}
        </form>
      )}
    </div>
  );
}

/** A slider that sends ONE request when you let go (mouse/touch up, key up, blur) instead of one per drag step:
 * in Live mode every request is an UPDATE on SingleStore, and overlapping ones were refused as "busy". */
export function RangeKnob({ min, max, value, onCommit }: { min: number; max: number; value: number; onCommit: (v: number) => Promise<void> }) {
  const [draft, setDraft] = useState<number | null>(null);
  const sending = useRef(false);
  const shown = draft ?? value;
  const commit = async () => {
    if (draft === null || sending.current) return;
    if (draft === value) {
      setDraft(null);
      return;
    }
    sending.current = true;
    try {
      await onCommit(draft);
    } finally {
      sending.current = false;
      setDraft(null);   // show the server's value again (unchanged if the request failed)
    }
  };
  return (
    <span className="rng">
      <input
        type="range"
        min={min}
        max={max}
        value={shown}
        onChange={(e) => setDraft(Number(e.target.value))}
        onPointerUp={commit}
        onKeyUp={commit}
        onBlur={commit}
      />
      <output>{String(shown)}</output>
    </span>
  );
}

function LogTab({ log }: { log: LogEntry[] }) {
  const end = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => log.slice(-250), [log]);
  useEffect(() => end.current?.scrollIntoView({ block: "end" }), [rows.length]);
  return (
    <div className="log">
      {rows.map((l, i) => (<div key={i} className={`ll l-${l.level}`}><time>{l.t}</time> {l.msg}</div>))}
      <div ref={end} />
    </div>
  );
}
