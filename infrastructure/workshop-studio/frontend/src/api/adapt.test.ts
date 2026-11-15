import { describe, expect, it } from 'vitest'
import { barsModel, evidenceError, jobStartedMs, nextVersionBaselines, previousTag, readyData, versionKey } from './adapt'
import type { Demo, LiveEvidence } from './types'
import { RECORDINGS } from '../dev/mockContent'

const versions = (tags: Record<string, string | null> | null): LiveEvidence => ({
  type: 'versions',
  title: 'V',
  data: tags && { environments: Object.entries(tags).map(([env, tag]) => ({ env: env as 'dev', label: env, tag })) },
})
const demo = (...evidence: LiveEvidence[]): Demo => ({ scenario: 's', status: 'running', message: '', steps: [{ id: 'one', status: 'running', actions: [], evidence }] })

describe('real payload helpers', () => {
  it('reads the job start as epoch seconds', () => {
    expect(jobStartedMs({ started: 1794745424.7256584 })).toBeCloseTo(1794745424725.6584, 0)
  })

  it('finds the error sentence a failed provider leaves behind', () => {
    expect(evidenceError({ error: 'We could not read this just now.' })).toBe('We could not read this just now.')
    expect(evidenceError(null)).toBeNull()
    expect(evidenceError({ value: 1, unit: '', compare: null })).toBeNull()
    expect(readyData({ data: { error: 'x' } })).toBeNull()
  })

  it('turns recorded bars (parallel arrays) into a model', () => {
    const frame = Object.values(RECORDINGS['switchover-date']!)[0]!.frames.at(-1)!
    const data = frame.evidence.find((e) => e && 'series' in e)!
    const m = barsModel(data as never)
    expect(m.labels).toHaveLength(14)
    expect(m.series.find((s) => s.name === 'readable')).toMatchObject({ state: 'good' })
    expect(m.total(0)).toBe(m.series.reduce((a, s) => a + s.values[0]!, 0))
    // short and null values count as zero
    const odd = barsModel({ labels: ['a', 'b', 3], x_label: '', series: [{ name: '', state: 'neutral', values: [1, null as never] }] })
    expect(odd.labels).toEqual(['a', 'b', '3'])
    expect(odd.series[0]!.values).toEqual([1, 0, 0])
  })

  it('remembers the first versions it sees per step and forgets them when the step restarts', () => {
    const key = versionKey('s', 'one', 0)
    let b = nextVersionBaselines({}, demo(versions(null)))
    expect(b).toEqual({})
    b = nextVersionBaselines(b, demo(versions({ dev: 'v1', val: 'v1', prd: null })))
    expect(b[key]).toEqual({ dev: 'v1', val: 'v1', prd: null })
    const later = nextVersionBaselines(b, demo(versions({ dev: 'v2', val: 'v1', prd: null })))
    expect(later).toBe(b) // the baseline is what it was when the step started
    expect(previousTag(b[key], 'dev')).toBe('v1')
    expect(previousTag(b[key], 'prd')).toBeNull()
    expect(previousTag(undefined, 'dev')).toBeUndefined()
    expect(nextVersionBaselines(b, demo(versions(null)))).toEqual({}) // step run again
    expect(nextVersionBaselines(b, null)).toEqual({})
  })
})
