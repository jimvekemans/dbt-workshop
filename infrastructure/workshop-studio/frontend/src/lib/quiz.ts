import { useSyncExternalStore } from 'react'
import type { Catalog, FeatureStatus, Quiz, ScenarioKind } from '../api/types'

/**
 * The quiz: questions for the room. A scenario may carry one (asked before the scenario starts) and so may each of its
 * steps (asked before that stage of a flow). The presenter counts raised hands by tapping a choice, then reveals what
 * every choice leads to. What the room answered, the route the presenter picked and the clock live in this browser
 * only (`localStorage`), like the text size: the backend knows the questions, never the answers.
 */
export const QUIZ_KEY = 'studio-quiz-v1'
export const DEFAULT_BUDGET = 120
export const LETTERS = ['A', 'B', 'C', 'D'] as const

export interface QuizItem {
  /** `<scenario id>` for the question of a scenario, `<scenario id>/<step id>` for the question of one step. */
  key: string
  scenarioId: string
  stepId: string | null
  /** 0-based place of the step in its demo; null for the question of a scenario. */
  stepIndex: number | null
  stepCount: number
  groupId: string
  groupTitle: string
  scenarioTitle: string
  kind: ScenarioKind
  /** What the question is about in plain words; only shown once the answer is revealed. */
  feature: string | null
  status: FeatureStatus | null
  /** What really happened at the client; only shown once the answer is revealed. */
  reality: string | null
  quiz: Quiz
  /** Minutes of talking for the question, minutes for what is shown after it, and their sum. */
  talk: number
  demo: number
  minutes: number
}

export function itemKey(scenarioId: string, stepId?: string | null): string {
  return stepId ? `${scenarioId}/${stepId}` : scenarioId
}

function isQuiz(q: unknown): q is Quiz {
  const quiz = q as Quiz | null | undefined
  return !!quiz && typeof quiz.question === 'string' && Array.isArray(quiz.options) && quiz.options.length >= 2
}

/** Every question of the catalog, in the order of the home page (a scenario's own question before those of its steps). */
export function quizItems(catalog: Catalog | null): QuizItem[] {
  if (!catalog) return []
  const out: QuizItem[] = []
  for (const g of catalog.groups) {
    for (const s of g.scenarios) {
      const steps = s.steps ?? []
      const base = { scenarioId: s.id, groupId: g.id, groupTitle: g.title, scenarioTitle: s.title, kind: s.kind ?? 'scenario', stepCount: steps.length }
      if (isQuiz(s.quiz)) {
        const talk = s.quiz.minutes ?? 2
        out.push({ ...base, key: itemKey(s.id), stepId: null, stepIndex: null, feature: s.feature ?? null, status: s.status ?? null, reality: s.reality ?? null, quiz: s.quiz, talk, demo: s.minutes, minutes: talk + s.minutes })
      }
      steps.forEach((st, i) => {
        if (!isQuiz(st.quiz)) return
        const talk = st.quiz.minutes ?? 2
        // a stage has no time of its own: an even share of the flow's time
        const demo = Math.max(1, Math.round(s.minutes / steps.length))
        out.push({ ...base, key: itemKey(s.id, st.id), stepId: st.id, stepIndex: i, feature: st.feature ?? st.title, status: st.status ?? null, reality: st.reality ?? null, quiz: st.quiz, talk, demo, minutes: talk + demo })
      })
    }
  }
  return out
}

/** Where a question is asked, and where the proof of it is shown. */
export function questionHref(item: Pick<QuizItem, 'scenarioId' | 'stepId'>): string {
  return `#/quiz/${encodeURIComponent(item.scenarioId)}${item.stepId ? `/${encodeURIComponent(item.stepId)}` : ''}`
}
export function proofHref(item: Pick<QuizItem, 'scenarioId' | 'stepId'>): string {
  const id = encodeURIComponent(item.scenarioId)
  return item.stepId ? `#/s/${id}/demo?step=${encodeURIComponent(item.stepId)}` : `#/s/${id}/problem`
}

// ------------------------------------------------------------------ what the room answered (kept in this browser)
export interface Answer {
  /** Raised hands per choice, in the order of the choices. */
  votes: number[]
  /** The impact of every choice has been shown. */
  revealed: boolean
  /** The presenter chose not to ask this one. */
  skipped: boolean
}
export interface QuizState {
  /** The questions the presenter picked for this session (keys). Empty: no route, every question counts. */
  plan: string[]
  answers: Record<string, Answer>
  /** A scenario with a question opens with that question first. */
  opening: boolean
  /** Minutes the presenter has for the whole session. */
  budget: number
  /** When the clock was started (ms), or null. */
  startedAt: number | null
}

export const EMPTY_ANSWER: Answer = { votes: [], revealed: false, skipped: false }
const DEFAULTS: QuizState = { plan: [], answers: {}, opening: true, budget: DEFAULT_BUDGET, startedAt: null }

function clean(raw: unknown): QuizState {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Partial<QuizState>
  const answers: Record<string, Answer> = {}
  if (r.answers && typeof r.answers === 'object') {
    for (const [k, v] of Object.entries(r.answers)) {
      const a = (v ?? {}) as Partial<Answer>
      answers[k] = {
        votes: Array.isArray(a.votes) ? a.votes.map((n) => (Number.isFinite(n) && n > 0 ? Math.floor(n) : 0)) : [],
        revealed: a.revealed === true,
        skipped: a.skipped === true,
      }
    }
  }
  const budget = typeof r.budget === 'number' && Number.isFinite(r.budget) ? Math.min(600, Math.max(10, Math.round(r.budget))) : DEFAULT_BUDGET
  return {
    plan: Array.isArray(r.plan) ? [...new Set(r.plan.filter((k): k is string => typeof k === 'string'))] : [],
    answers,
    opening: r.opening !== false,
    budget,
    startedAt: typeof r.startedAt === 'number' && Number.isFinite(r.startedAt) ? r.startedAt : null,
  }
}

let memory: string | null = null // stands in for localStorage when it cannot be used (private window)
let cachedRaw: string | null | undefined
let cached: QuizState = DEFAULTS
const listeners = new Set<() => void>()

function readRaw(): string | null {
  try {
    return localStorage.getItem(QUIZ_KEY)
  } catch {
    return memory
  }
}

export function readQuiz(): QuizState {
  const raw = readRaw()
  if (raw === cachedRaw) return cached
  cachedRaw = raw
  try {
    cached = raw ? clean(JSON.parse(raw)) : DEFAULTS
  } catch {
    cached = DEFAULTS
  }
  return cached
}

function update(change: (s: QuizState) => QuizState): void {
  const raw = JSON.stringify(change(readQuiz()))
  memory = raw
  try {
    localStorage.setItem(QUIZ_KEY, raw)
  } catch {
    /* private window: the answers live as long as this page */
  }
  listeners.forEach((l) => l())
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  const onStorage = (e: StorageEvent) => (e.key === QUIZ_KEY || e.key === null) && listener()
  window.addEventListener('storage', onStorage)
  return () => {
    listeners.delete(listener)
    window.removeEventListener('storage', onStorage)
  }
}

export function useQuiz(): QuizState {
  return useSyncExternalStore(subscribe, readQuiz, () => DEFAULTS)
}

const withAnswer = (s: QuizState, key: string, change: (a: Answer) => Answer): QuizState => ({
  ...s,
  answers: { ...s.answers, [key]: change(s.answers[key] ?? EMPTY_ANSWER) },
})

export const quizActions = {
  /** One hand more (or less) for a choice; `count` is the number of choices of the question. */
  vote(key: string, index: number, delta: 1 | -1, count: number) {
    if (index < 0 || index >= count) return
    update((s) =>
      withAnswer(s, key, (a) => {
        const votes = Array.from({ length: count }, (_, i) => a.votes[i] ?? 0)
        votes[index] = Math.max(0, Math.min(99, (votes[index] ?? 0) + delta))
        return { ...a, votes, skipped: false }
      }),
    )
  },
  reveal(key: string) {
    update((s) => withAnswer(s, key, (a) => ({ ...a, revealed: true, skipped: false })))
  },
  skip(key: string) {
    update((s) => withAnswer(s, key, (a) => ({ ...a, skipped: true })))
  },
  /** Ask this question again from nothing. */
  again(key: string) {
    update((s) => {
      const answers = { ...s.answers }
      delete answers[key]
      return { ...s, answers }
    })
  },
  setPlanned(keys: string[], on: boolean) {
    update((s) => {
      const drop = new Set(keys)
      const rest = s.plan.filter((k) => !drop.has(k))
      return { ...s, plan: on ? [...rest, ...keys] : rest }
    })
  },
  clearPlan() {
    update((s) => ({ ...s, plan: [] }))
  },
  /** A new session: no answers, no clock. The route and the settings stay. */
  clearAnswers() {
    update((s) => ({ ...s, answers: {}, startedAt: null }))
  },
  setOpening(on: boolean) {
    update((s) => ({ ...s, opening: on }))
  },
  setBudget(minutes: number) {
    update((s) => clean({ ...s, budget: minutes }))
  },
  startClock(now: number = Date.now()) {
    update((s) => ({ ...s, startedAt: now }))
  },
  stopClock() {
    update((s) => ({ ...s, startedAt: null }))
  },
}

// ------------------------------------------------------------------ what follows from the answers
export interface Tally {
  votes: number[]
  total: number
  /** The choices with the most hands (several on a tie, none without hands). */
  top: number[]
  /** A clear majority for the platform's choice. */
  agrees: boolean
}

export function tally(quiz: Quiz, answer: Answer | undefined): Tally {
  const votes = quiz.options.map((_, i) => answer?.votes[i] ?? 0)
  const total = votes.reduce((a, b) => a + b, 0)
  const max = Math.max(0, ...votes)
  const top = max > 0 ? votes.flatMap((v, i) => (v === max ? [i] : [])) : []
  return { votes, total, top, agrees: top.length === 1 && quiz.options[top[0]!]?.platform === true }
}

/** "A 3 · B 7": what the room chose, in a few characters. */
export function votesText(t: Tally): string {
  return t.votes.map((v, i) => `${LETTERS[i]} ${v}`).join(' · ')
}

/** Asked and answered: the impacts were shown. */
export function isPlayed(state: QuizState, key: string): boolean {
  return state.answers[key]?.revealed === true
}

/** Opening the scenario should ask this first: the impacts were not shown yet and it was not skipped. */
export function isWaiting(state: QuizState, key: string): boolean {
  const a = state.answers[key]
  return !a || (!a.revealed && !a.skipped)
}

/** Still to play: not asked and not skipped. */
const isOpen = (state: QuizState, i: QuizItem) => !isPlayed(state, i.key) && state.answers[i.key]?.skipped !== true

/** The questions that count for the session: the picked route, or every question when nothing is picked. */
export function routeItems(items: QuizItem[], state: QuizState): QuizItem[] {
  const picked = new Set(state.plan)
  const mine = items.filter((i) => picked.has(i.key))
  return mine.length > 0 ? mine : items
}

/** The next question of the route that has not been played: the first one after `afterKey`, else the first one at all. */
export function nextItem(items: QuizItem[], state: QuizState, afterKey?: string | null): QuizItem | null {
  const open = (i: QuizItem) => isOpen(state, i) && i.key !== afterKey
  const route = routeItems(items, state)
  const at = afterKey ? items.findIndex((i) => i.key === afterKey) : -1
  const order = new Map(items.map((i, n) => [i.key, n]))
  return route.find((i) => open(i) && (order.get(i.key) ?? 0) > at) ?? route.find(open) ?? null
}

export interface Score {
  /** Questions answered with at least one hand counted. */
  counted: number
  /** ... of which the room clearly chose what the platform does. */
  agreed: number
  played: number
}

export function score(items: QuizItem[], state: QuizState): Score {
  let counted = 0
  let agreed = 0
  let played = 0
  for (const i of items) {
    if (!isPlayed(state, i.key)) continue
    played += 1
    const t = tally(i.quiz, state.answers[i.key])
    if (t.total === 0) continue
    counted += 1
    if (t.agrees) agreed += 1
  }
  return { counted, agreed, played }
}

export interface Timing {
  /** True when the presenter picked a route (otherwise the numbers are about every question). */
  picked: boolean
  questions: number
  plannedMinutes: number
  /** Minutes of the route's questions that are not played yet. */
  toGoMinutes: number
  budget: number
  /** Minutes since the clock started; null while it does not run. */
  elapsed: number | null
  /** Minutes left over (negative: too much planned): against what is left of the session once the clock runs. */
  slack: number
}

/**
 * Minutes a set of questions takes, vraag and demo together. The steps of a demo run in order, so a question about
 * step 5 also costs the demo of steps 1 to 4: per scenario the demo counts up to the last step that is asked about.
 * With `toGo` only what is still to play counts (and the steps before the last played one are behind us).
 */
export function routeMinutes(route: QuizItem[], state: QuizState, toGo = false): number {
  let total = 0
  const staged = new Map<string, QuizItem[]>()
  for (const i of route) {
    if (i.stepIndex === null) {
      if (!toGo || isOpen(state, i)) total += i.minutes
    } else {
      staged.set(i.scenarioId, [...(staged.get(i.scenarioId) ?? []), i])
    }
  }
  for (const steps of staged.values()) {
    const wanted = toGo ? steps.filter((i) => isOpen(state, i)) : steps
    if (wanted.length === 0) continue
    const last = Math.max(...wanted.map((i) => i.stepIndex ?? 0))
    const behind = toGo ? Math.max(-1, ...steps.filter((i) => !isOpen(state, i)).map((i) => i.stepIndex ?? 0)) : -1
    total += wanted.reduce((a, i) => a + i.talk, 0) + (steps[0]?.demo ?? 0) * Math.max(0, last - behind)
  }
  return total
}

export function timing(items: QuizItem[], state: QuizState, now: number = Date.now()): Timing {
  const picked = new Set(state.plan)
  const mine = items.filter((i) => picked.has(i.key))
  const route = mine.length > 0 ? mine : items
  const plannedMinutes = routeMinutes(route, state)
  const toGoMinutes = routeMinutes(route, state, true)
  const elapsed = state.startedAt === null ? null : Math.max(0, Math.floor((now - state.startedAt) / 60000))
  const slack = elapsed === null ? state.budget - plannedMinutes : state.budget - elapsed - toGoMinutes
  return { picked: mine.length > 0, questions: route.length, plannedMinutes, toGoMinutes, budget: state.budget, elapsed, slack }
}
