import { ApiError } from './types'
import type { Backend, Catalog, InputValues, Mode, Scenario, ServerState, Subscription } from './types'

const HEADERS = { 'X-Studio': '1' }

async function plainSentence(res: Response): Promise<string> {
  const text = await res.text()
  try {
    const body = JSON.parse(text) as { detail?: unknown; message?: unknown }
    const d = body.detail ?? body.message
    if (typeof d === 'string') return d
    // The backend answers a broken content set with {detail: {message, problems}}.
    if (d && typeof d === 'object' && typeof (d as { message?: unknown }).message === 'string') return (d as { message: string }).message
  } catch {
    /* not JSON */
  }
  return text.trim() || `De Studio antwoordde met een fout (${res.status}).`
}

async function call<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
  let res: Response
  try {
    res = await fetch(`/api${path}`, {
      method,
      headers: body === undefined ? HEADERS : { ...HEADERS, 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  } catch {
    throw new ApiError('De Studio is nu niet bereikbaar.', 0)
  }
  if (!res.ok) throw new ApiError(await plainSentence(res), res.status)
  return (await res.json()) as T
}

/** SSE subscription with manual reconnect (EventSource gives up after some errors) and a connection flag. */
export function subscribeEvents(sub: Subscription, url = '/api/events'): () => void {
  let es: EventSource | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  let attempt = 0
  let closed = false

  const open = () => {
    if (closed) return
    es = new EventSource(url)
    es.addEventListener('open', () => {
      attempt = 0
      sub.onConnection(true)
    })
    es.addEventListener('state', (ev) => {
      try {
        sub.onState(JSON.parse((ev as MessageEvent<string>).data) as ServerState)
        sub.onConnection(true)
      } catch {
        /* ignore a malformed event */
      }
    })
    es.addEventListener('error', () => {
      sub.onConnection(false)
      es?.close()
      es = null
      attempt += 1
      timer = setTimeout(open, Math.min(1000 * 2 ** Math.min(attempt, 3), 8000))
    })
  }
  open()
  return () => {
    closed = true
    if (timer) clearTimeout(timer)
    es?.close()
  }
}

export const httpBackend: Backend = {
  getCatalog: () => call<Catalog>('GET', '/catalog'),
  getScenario: (id) => call<Scenario>('GET', `/scenarios/${encodeURIComponent(id)}`),
  getState: () => call<ServerState>('GET', '/state'),
  prepare: (id) => call<ServerState>('POST', `/scenarios/${encodeURIComponent(id)}/prepare`, {}),
  runStep: (id, step, inputs: InputValues) =>
    call<ServerState>('POST', `/scenarios/${encodeURIComponent(id)}/steps/${encodeURIComponent(step)}/run`, { inputs }),
  cleanup: () => call<ServerState>('POST', '/cleanup', {}),
  cancel: () => call<ServerState>('POST', '/cancel', {}),
  setMode: (mode: Mode) => call<ServerState>('POST', '/mode', { mode }),
  setFeeder: (running: boolean) => call<ServerState>('POST', '/feeder', { running }),
  subscribe: (sub) => subscribeEvents(sub),
}

/** Picks the mock only in dev builds or when VITE_MOCK=1; otherwise the mock is tree-shaken out of the bundle. */
export async function createBackend(): Promise<Backend> {
  if (import.meta.env.DEV || import.meta.env.VITE_MOCK === '1') {
    const wantMock = import.meta.env.VITE_MOCK === '1' || new URLSearchParams(window.location.search).has('mock')
    if (wantMock) {
      const { createMockBackend } = await import('../dev/mock')
      return createMockBackend(new URLSearchParams(window.location.search))
    }
  }
  return httpBackend
}
