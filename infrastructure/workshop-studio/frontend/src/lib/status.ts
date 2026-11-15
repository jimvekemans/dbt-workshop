import type { ProgressStatus, RunState } from '../api/types'
import type { StatusState } from '../components/Status'

/** The state codes of tasks, jobs and actions as a status mark; `text` is the backend's own words when it has them. */
export function taskStatus(s: RunState | ProgressStatus | 'waiting' | 'manual', text?: string): { state: StatusState; word: string } {
  const word = (fallback: string) => text || fallback
  switch (s) {
    case 'done':
      return { state: 'ok', word: word('Done') }
    case 'failed':
      return { state: 'bad', word: word('Failed') }
    case 'running':
      return { state: 'running', word: word('Working') }
    case 'manual':
      return { state: 'idle', word: word('Waiting for a person') }
    case 'skipped':
      return { state: 'idle', word: word('Skipped') }
    default:
      return { state: 'idle', word: word('Not started') }
  }
}
