import type { ServerState } from '../api/types'
import { envWords } from './env'

export interface Health {
  total: number
  notRunning: number
  /** The plain line about the practice system when it is not ready; null when it is. */
  baselineLine: string | null
  leftovers: number
  /** Something the presenter should look at: Details opens by itself. */
  attention: boolean
  /** "Demo system: ready", the warning sentence, or '' when only the practice-system line is shown. */
  headline: string
  /** The headline is a warning (a thing is not running). */
  warn: boolean
}

/** What the one quiet status row on the home page says, from the server state. */
export function health(server: ServerState): Health {
  const total = server.stack.length
  const notRunning = server.stack.filter((s) => s.status !== 'up').length
  const b = server.baseline
  let baselineLine: string | null = null
  if (server.waiting) baselineLine = server.waiting.text
  else if (b.status === 'preparing') {
    const now = b.progress.find((p) => p.status === 'running') ?? b.progress.find((p) => p.status === 'todo')
    baselineLine = `Getting the practice system ready${now ? `: ${envWords(now.label)}` : ''}`
  } else if (b.status === 'failed') baselineLine = 'The practice system could not be made ready'
  else if (b.status === 'missing') baselineLine = 'The practice system has not been checked yet'
  const leftovers = server.leftovers.length
  const headline = notRunning > 0 ? `${notRunning} of ${total} things the demos need are not running` : baselineLine ? '' : 'Demo system: ready'
  return { total, notRunning, baselineLine, leftovers, attention: notRunning > 0 || baselineLine !== null || leftovers > 0, headline, warn: notRunning > 0 }
}
