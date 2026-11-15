import { describe, expect, it } from 'vitest'
import { addDays, compareWords, diffDays, formatDay, formatDuration, formatNumber, plural } from './format'

describe('day helpers', () => {
  it('adds and subtracts days across month ends', () => {
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28')
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01')
    expect(diffDays('2026-03-11', '2026-03-18')).toBe(7)
    expect(diffDays('2026-03-18', '2026-03-11')).toBe(-7)
  })
  it('spells out the day including the weekday, without time zone drift', () => {
    expect(formatDay('2026-03-18', 'long')).toBe('Wednesday 18 March 2026')
    expect(formatDay('2026-03-18', 'short')).toBe('Wed 18 Mar')
    expect(formatDay('2026-03-18', 'weekday')).toBe('Wednesday')
    expect(formatDay('2026-03-18', 'noyear')).toBe('Wednesday 18 March')
  })
})

describe('number formatting', () => {
  it('groups thousands and trims decimals', () => {
    expect(formatNumber(1234567)).toBe('1,234,567')
    expect(formatNumber(2.456, 2)).toBe('2.46')
    expect(formatNumber(Number.NaN)).toBe('–')
  })
  it('pluralises', () => {
    expect(plural(1, 'delivery', 'deliveries')).toBe('1 delivery')
    expect(plural(0, 'delivery', 'deliveries')).toBe('0 deliveries')
    expect(plural(1200, 'delivery', 'deliveries')).toBe('1,200 deliveries')
  })
  it('formats durations', () => {
    expect(formatDuration(4)).toBe('4 s')
    expect(formatDuration(60)).toBe('1 min')
    expect(formatDuration(95)).toBe('1 min 35 s')
  })
  it('compares in words', () => {
    expect(compareWords(5, 3, 'last week')).toBe('2 more than last week (3)')
    expect(compareWords(1, 4, 'last week')).toBe('3 fewer than last week (4)')
    expect(compareWords(2, 2, 'last week')).toBe('The same as last week (2)')
  })
})

describe('usualText, formatAgo, daysAgoText', () => {
  it('rounds to 5 seconds and says minutes above 90 seconds', async () => {
    const { usualText } = await import('./format')
    expect(usualText(null)).toBeNull()
    expect(usualText(0)).toBeNull()
    expect(usualText(2)).toBe('usually about 5 s')
    expect(usualText(23)).toBe('usually about 25 s')
    expect(usualText(87)).toBe('usually about 85 s')
    expect(usualText(91)).toBe('usually about 2 minutes')
    expect(usualText(140)).toBe('usually about 2 minutes')
  })
  it('says "a moment ago" and minutes for recent times', async () => {
    const { formatAgo } = await import('./format')
    const now = Date.parse('2026-11-15T10:00:00')
    expect(formatAgo('2026-11-15T09:59:40', now)).toBe('a moment ago')
    expect(formatAgo('2026-11-15T09:53:00', now)).toBe('7 minutes ago')
  })
  it('counts days back from today', async () => {
    const { daysAgoText } = await import('./format')
    expect(daysAgoText('2026-11-15', '2026-11-15')).toBe('today')
    expect(daysAgoText('2026-11-14', '2026-11-15')).toBe('yesterday')
    expect(daysAgoText('2026-11-11', '2026-11-15')).toBe('4 days ago')
  })
})
