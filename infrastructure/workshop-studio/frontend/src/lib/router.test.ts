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
  it('knows the quiz page, a question of a scenario and a question of one step', () => {
    expect(parseHash('#/quiz')).toEqual({ name: 'quiz' })
    expect(parseHash('#/quiz/skip-one-table')).toEqual({ name: 'question', id: 'skip-one-table' })
    expect(parseHash('#/quiz/flow-test/check')).toEqual({ name: 'question', id: 'flow-test', step: 'check' })
    expect(parseHash('#/quiz/a/b/c')).toEqual({ name: 'unknown' })
    expect(toHash({ name: 'question', id: 'flow-test', step: 'check' })).toBe('#/quiz/flow-test/check')
    expect(toHash({ name: 'quiz' })).toBe('#/quiz')
  })
  it('carries the demo step a question points at', () => {
    const r = { name: 'scenario', id: 'flow-test', part: 'demo', step: 'check' } as const
    expect(toHash(r)).toBe('#/s/flow-test/demo?step=check')
    expect(parseHash(toHash(r))).toEqual(r)
  })
  it('numbers the parts', () => {
    expect(partIndex('problem')).toBe(0)
    expect(partIndex('demo')).toBe(3)
  })
})
