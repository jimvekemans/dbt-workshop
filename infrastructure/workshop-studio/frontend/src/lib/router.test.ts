import { describe, expect, it } from 'vitest'
import { parseHash, toHash, partIndex } from './router'

describe('hash router', () => {
  it('maps empty hashes to home and unknown ones to "unknown" (the app then says so)', () => {
    expect(parseHash('')).toEqual({ name: 'home' })
    expect(parseHash('#/')).toEqual({ name: 'home' })
    expect(parseHash('#/nonsense/x')).toEqual({ name: 'unknown' })
  })
  it('parses scenario routes with a part', () => {
    expect(parseHash('#/s/switchover-date/demo')).toEqual({ name: 'scenario', id: 'switchover-date', part: 'demo' })
    expect(parseHash('#/s/x/idea')).toEqual({ name: 'scenario', id: 'x', part: 'idea' })
  })
  it('falls back to the first part when it is missing or wrong, and asks for the address to be repaired', () => {
    expect(parseHash('#/s/x')).toEqual({ name: 'scenario', id: 'x', part: 'problem', repair: true })
    expect(parseHash('#/s/x/bogus')).toEqual({ name: 'scenario', id: 'x', part: 'problem', repair: true })
    expect(parseHash('#/s/x/idea')).not.toHaveProperty('repair')
  })
  it('remembers the scenario a visitor came from', () => {
    expect(parseHash('#/s/x/problem?from=y')).toEqual({ name: 'scenario', id: 'x', part: 'problem', from: 'y' })
  })
  it('round-trips', () => {
    const r = { name: 'scenario', id: 'one-clear-mail', part: 'code' } as const
    expect(parseHash(toHash(r))).toEqual(r)
    expect(toHash({ name: 'home' })).toBe('#/')
  })
  it('numbers the parts', () => {
    expect(partIndex('problem')).toBe(0)
    expect(partIndex('demo')).toBe(3)
  })
})
