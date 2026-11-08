import { useCallback, useEffect, useMemo, useState } from "react";
import { api } from "./api";
import { Drawer, type Tab } from "./components/Drawer";
import { Header } from "./components/Header";
import { MapCanvas } from "./components/MapCanvas";
import { Timeline } from "./components/Timeline";
import { keyAction } from "./lib/keys";
import { useStudio } from "./store";
import type { GroupId, ScenarioDetail } from "./types";

export function App() {
  const studio = useStudio();
  const { world, model } = studio;
  const state = model.state;

  const [zoom, setZoom] = useState<{ target: string | null; nonce: number }>({ target: null, nonce: 0 });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("scenario");
  const [detail, setDetail] = useState<ScenarioDetail | null>(null);
  const [previewStep, setPreviewStep] = useState<number | null>(null);
  const [ownerFocus, setOwnerFocus] = useState<string | null>(null);
  const [large, setLarge] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  useEffect(() => {
    if (!hint) return;
    const t = setTimeout(() => setHint(null), 4000);
    return () => clearTimeout(t);
  }, [hint]);

  const zoomTo = useCallback((target: string | null) => setZoom((z) => ({ target, nonce: z.nonce + 1 })), []);

  // scenario detail (steps, rendered commands) follows the active scenario
  const scenarioId = state?.scenarioId ?? null;
  useEffect(() => {
    setPreviewStep(null);
    if (!scenarioId) {
      setDetail(null);
      return;
    }
    let live = true;
    api.scenario(scenarioId).then((d) => live && setDetail(d)).catch(() => live && setDetail(null));
    return () => {
      live = false;
    };
  }, [scenarioId]);

  // injecting zooms back out so the ripple is visible from the overview
  useEffect(() => {
    if (scenarioId) zoomTo(null);
  }, [scenarioId, zoomTo]);

  useEffect(() => {
    document.documentElement.dataset.large = large ? "1" : "0";
  }, [large]);

  const stepIndex = state?.stepIndex ?? 0;
  const shownStep = detail ? detail.steps[Math.min(previewStep ?? stepIndex, detail.steps.length - 1)] : undefined;
  const highlight = useMemo(
    () => new Set<string>(tab === "playbook" || tab === "code" ? (shownStep?.highlight ?? []) : []),
    [tab, shownStep],
  );

  const previewAndZoom = useCallback(
    (n: number | null) => {
      setPreviewStep(n);
      const step = n === null ? undefined : detail?.steps[n];
      if (step?.zoom) zoomTo(step.zoom);
    },
    [detail, zoomTo],
  );

  const runStep = useCallback(() => {
    if (!state?.scenarioId || state.busy || state.phase === "resolved") return;
    const step = detail?.steps[state.stepIndex];
    if (step?.zoom) zoomTo(step.zoom);
    setPreviewStep(null);
    studio.run(api.nextStep);
  }, [state, detail, zoomTo, studio]);

  const liveExec = state?.mode === "live" && !state?.dryRun;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const a = keyAction(e.key, e.target as HTMLElement | null, liveExec);
      if (!a || e.metaKey || e.ctrlKey || e.altKey) return;
      if (a.type === "next") runStep();
      else if (a.type === "overview") zoomTo(null);
      else if (a.type === "zoom") zoomTo(a.target);
      else if (a.type === "tick" && !state?.busy) studio.run(api.tick);
      else if (a.type === "blocked")
        setHint(
          a.what === "next"
            ? "Live mode: → does not run steps against the stack - click ▶ Run step (live)."
            : "Live mode: T does not advance the sprint against the stack - click ⏭ Next day.",
        );
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [runStep, zoomTo, studio, state?.busy, liveExec]);

  const onZoomGroup = useCallback((g: GroupId) => zoomTo(g), [zoomTo]);

  if (!world || !state) {
    return (
      <div className="boot">
        <h1>Workshop Studio</h1>
        <p>{studio.error ?? "Loading the map…"}</p>
      </div>
    );
  }

  const selected = selectedId ? (world.nodes.find((n) => n.id === selectedId) ?? null) : null;
  return (
    <div className="app">
      <Header studio={studio} state={state} large={large} onLarge={() => setLarge((l) => !l)} />
      {studio.error && (
        <div className="banner" role="alert">
          {studio.error} <button className="linkbtn" onClick={studio.clearError}>dismiss</button>
        </div>
      )}
      {world.contentError && (
        <div className="banner" role="alert">
          Scenario YAML not reloaded (the previous version stays active): {world.contentError}
        </div>
      )}
      {hint && (
        <div className="hintbar" role="status">
          {hint}
        </div>
      )}
      <main className="main">
        <section className="canvas" aria-label="Data value chain map">
          <div className="zoombar" role="group" aria-label="Zoom">
            <button className={zoom.target === null ? "on" : ""} onClick={() => zoomTo(null)} title="Esc">⤢ Overview</button>
            <button className={zoom.target === "sources" ? "on" : ""} onClick={() => zoomTo("sources")} title="1">① Sources</button>
            <button className={zoom.target === "platform" ? "on" : ""} onClick={() => zoomTo("platform")} title="2">② Platform</button>
            <button className={zoom.target === "consumers" ? "on" : ""} onClick={() => zoomTo("consumers")} title="3">③ Consumers</button>
          </div>
          <MapCanvas
            groups={world.groups}
            nodes={world.nodes}
            edges={world.edges}
            state={state}
            pulse={model.pulse}
            selectedId={selectedId}
            highlight={highlight}
            ownerFocus={ownerFocus}
            zoom={zoom}
            onSelect={(id) => {
              setSelectedId(id);
              if (id) setTab("node");
            }}
            onZoomGroup={onZoomGroup}
          />
          <div className="legend" aria-hidden>
            <span><i className="dot d-ok" /> ok</span>
            <span><i className="dot d-warn" /> warning</span>
            <span><i className="dot d-fail" /> failing</span>
            <span><i className="dot d-idle" /> idle</span>
          </div>
        </section>
        <Drawer
          studio={studio}
          world={world}
          state={state}
          log={model.log}
          tab={tab}
          setTab={setTab}
          zoomTarget={zoom.target}
          selected={selected}
          detail={detail}
          previewStep={previewStep}
          setPreviewStep={previewAndZoom}
          ownerFocus={ownerFocus}
          setOwnerFocus={setOwnerFocus}
          runStep={runStep}
        />
      </main>
      <Timeline studio={studio} world={world} state={state} />
    </div>
  );
}
