import { describe, expect, it } from 'vitest'
import { health } from './health'
import { baseServer } from '../test/harness'
import type { ServerState, StackService } from '../api/types'

const svc = (id: string, status: StackService['status'] = 'up'): StackService => ({ id, label: id, status })
const server = (over: Partial<ServerState>): ServerState => ({ ...baseServer, ...over })

describe('health: the one status row', () => {
  it('says everything is running and asks for no attention', () => {
    const h = health(server({ stack: [svc('a'), svc('b')] }))
    expect(h).toMatchObject({ headline: 'Demo system: ready', warn: false, attention: false, baselineLine: null })
  })
  it('counts what is not running (starting counts too) in the warning sentence', () => {
    const h = health(server({ stack: [svc('a'), svc('b', 'down'), svc('c', 'starting')] }))
    expect(h.headline).toBe('2 of 3 things the demos need are not running')
    expect(h).toMatchObject({ warn: true, attention: true })
  })
  it('shows the practice system line with the current check, in the audience\'s environment words', () => {
    const progress = [{ label: 'Demo data in dev', status: 'done' as const }, { label: 'A numbered version running in val', status: 'running' as const }, { label: 'Tables built once in prd', status: 'todo' as const }]
    const h = health(server({ stack: [svc('a')], baseline: { status: 'preparing', detail: '', progress } }))
    expect(h.baselineLine).toBe('Getting the practice system ready: A numbered version running in validation')
    expect(h.headline).toBe('')
    expect(h.attention).toBe(true)
  })
  it('asks for attention for a failed or missing baseline and for leftovers', () => {
    expect(health(server({ stack: [svc('a')], baseline: { status: 'failed', detail: 'x', progress: [] } })).baselineLine).toMatch(/could not be made ready/)
    expect(health(server({ stack: [svc('a')], baseline: { status: 'missing', detail: '', progress: [] } })).attention).toBe(true)
    expect(health(server({ stack: [svc('a')], leftovers: [{ label: 'x', from_scenario: null }] })).attention).toBe(true)
  })
})
