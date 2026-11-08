import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { Studio } from "../store";
import type { StudioState, World } from "../types";

export function Timeline({ studio, world, state }: { studio: Studio; world: World; state: StudioState }) {
  const [playing, setPlaying] = useState(false);
  const days = world.sprint.days;
  const done = state.sprintDay >= days.length;
  const busyRef = useRef(state.busy);
  busyRef.current = state.busy;

  useEffect(() => {
    if (!playing) return;
    if (done) {
      setPlaying(false);
      return;
    }
    const t = setInterval(() => {
      if (!busyRef.current) studio.run(api.tick);
    }, 2200);
    return () => clearInterval(t);
  }, [playing, done, studio]);

  const current = days[Math.max(0, state.sprintDay - 1)];
  const live = state.mode === "live";
  return (
    <footer className="tl">
      <div className="tl-ctl">
        <button className="btn" onClick={() => setPlaying((p) => !p)} disabled={done && !playing} title="Auto-play the sprint">{playing ? "⏸" : "▶"}</button>
        <button className="btn" onClick={() => studio.run(api.tick)} disabled={done || !!state.busy} title="Next sprint day (T)">⏭ Next day</button>
      </div>
      <ol className="tl-days">
        {days.map((d) => (
          <li key={d.day} className={d.day <= state.sprintDay ? "past" : ""} aria-current={d.day === state.sprintDay ? "step" : undefined}>
            <span className="tl-dot">{d.day}</span>
            {d.milestone && (
              <span
                className={`tl-mile${live && d.live_note ? " sim-only" : ""}`}
                title={live && d.live_note ? `${d.title}\nLive: ${d.live_note}` : d.title}
              >
                ⚑ {d.milestone}
                {live && d.live_note ? " (sim)" : ""}
              </span>
            )}
          </li>
        ))}
      </ol>
      <div className="tl-now">
        {state.sprintDay === 0 ? "Sprint not started - press Next day (T) or inject a scenario" : `Day ${current.day}/${days.length}: ${current.title}`}
        {live && state.sprintDay > 0 && current.live_note && <span className="tl-live"> · Live: {current.live_note}</span>}
      </div>
    </footer>
  );
}
