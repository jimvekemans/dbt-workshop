import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import { api } from "./api";
import { applyEvent, initialModel, seed, type Model } from "./lib/reducer";
import type { StudioState, WsEvent, World } from "./types";

type Action = { type: "event"; ev: WsEvent } | { type: "seed"; state: StudioState; log: World["log"] };

function reduce(model: Model, a: Action): Model {
  return a.type === "seed" ? seed(a.state, a.log) : applyEvent(model, a.ev);
}

export interface Studio {
  world: World | null;
  model: Model;
  connected: boolean;
  error: string | null;
  /** run an API call; failures surface in the header instead of throwing into React */
  run: (fn: () => Promise<StudioState>) => Promise<void>;
  clearError: () => void;
}

export function useStudio(): Studio {
  const [world, setWorld] = useState<World | null>(null);
  const [model, dispatch] = useReducer(reduce, initialModel);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const closed = useRef(false);
  const contentVersion = useRef<number | null>(null);

  useEffect(() => {
    closed.current = false;
    let ws: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | undefined;

    const load = async () => {
      try {
        const w = await api.world();
        setWorld(w);
        contentVersion.current = w.state.contentVersion;
        dispatch({ type: "seed", state: w.state, log: w.log });
        setError(null);
      } catch (e) {
        setError(`Backend unreachable: ${(e as Error).message}`);
        retry = setTimeout(load, 2000);
        return;
      }
      connect();
    };

    const connect = () => {
      const proto = location.protocol === "https:" ? "wss" : "ws";
      ws = new WebSocket(`${proto}://${location.host}/ws`);
      ws.onopen = () => setConnected(true);
      ws.onmessage = (m) => {
        const ev = JSON.parse(m.data) as WsEvent;
        dispatch({ type: "event", ev });
        // the scenario YAML was reloaded (another tab refreshed after an edit): fetch the new map + catalogue
        if (ev.type === "state" && contentVersion.current !== null && ev.state.contentVersion !== contentVersion.current) {
          contentVersion.current = ev.state.contentVersion;
          api.world().then(setWorld).catch(() => undefined);
        }
      };
      ws.onclose = () => {
        setConnected(false);
        if (!closed.current) retry = setTimeout(connect, 1500);
      };
    };

    load();
    return () => {
      closed.current = true;
      clearTimeout(retry);
      ws?.close();
    };
  }, []);

  const run = useCallback(async (fn: () => Promise<StudioState>) => {
    try {
      const state = await fn();
      dispatch({ type: "event", ev: { type: "state", state } });
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  return { world, model, connected, error, run, clearError: () => setError(null) };
}
