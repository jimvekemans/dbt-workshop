import { describe, expect, it } from 'vitest'
import { addDays, compareWords, diffDays, formatClock, formatDay, formatDuration, formatNumber, formatWhen, formatWhenSeconds, plural } from './format'

describe('day helpers', () => {
  it('adds and subtracts days across month ends', () => {
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28')
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01')
    expect(diffDays('2026-03-11', '2026-03-18')).toBe(7)
    expect(diffDays('2026-03-18', '2026-03-11')).toBe(-7)
  })
  it('spells out the day including the weekday, without time zone drift', () => {
    expect(formatDay('2026-03-18', 'long')).toBe('woensdag 18 maart 2026')
    expect(formatDay('2026-03-18', 'short')).toBe('wo 18 mrt')
    expect(formatDay('2026-03-18', 'weekday')).toBe('woensdag')
    expect(formatDay('2026-03-18', 'noyear')).toBe('woensdag 18 maart')
  })
})

describe('number formatting', () => {
  it('groups thousands and trims decimals', () => {
    expect(formatNumber(1234567)).toBe('1.234.567')
    expect(formatNumber(2.456, 2)).toBe('2,46')
    expect(formatNumber(Number.NaN)).toBe('–')
  })
  it('uses a point for thousands and a comma for decimals, as in Belgium', () => {
    expect(formatNumber(3432)).toBe('3.432')
    expect(formatNumber(92.8)).toBe('92,8')
    expect(formatNumber(1234.5, 2)).toBe('1.234,5')
  })
  it('writes a moment as "21 november, 18:06" (a local time, no year)', () => {
    expect(formatWhen('2026-11-21T18:06:00')).toBe('21 november, 18:06')
    expect(formatWhenSeconds('2026-11-21T18:06:09')).toBe('21 november, 18:06:09')
    expect(formatClock('2026-11-21T09:05:00')).toBe('09:05')
  })
  it('pluralises', () => {
    expect(plural(1, 'levering', 'leveringen')).toBe('1 levering')
    expect(plural(0, 'levering', 'leveringen')).toBe('0 leveringen')
    expect(plural(1200, 'levering', 'leveringen')).toBe('1.200 leveringen')
  })
  it('formats durations', () => {
    expect(formatDuration(4)).toBe('4 s')
    expect(formatDuration(60)).toBe('1 min')
    expect(formatDuration(95)).toBe('1 min 35 s')
  })
  it('compares in words', () => {
    expect(compareWords(5, 3, 'vorige week')).toBe('2 meer dan vorige week (3)')
    expect(compareWords(1, 4, 'vorige week')).toBe('3 minder dan vorige week (4)')
    expect(compareWords(2, 2, 'vorige week')).toBe('Evenveel als vorige week (2)')
  })
})

describe('usualText, formatAgo, daysAgoText', () => {
  it('rounds to 5 seconds and says minutes above 90 seconds', async () => {
    const { usualText } = await import('./format')
    expect(usualText(null)).toBeNull()
    expect(usualText(0)).toBeNull()
    expect(usualText(2)).toBe('meestal ongeveer 5 s')
    expect(usualText(23)).toBe('meestal ongeveer 25 s')
    expect(usualText(87)).toBe('meestal ongeveer 85 s')
    expect(usualText(91)).toBe('meestal ongeveer 2 minuten')
    expect(usualText(140)).toBe('meestal ongeveer 2 minuten')
  })
  it('says "zonet" and minutes for recent times', async () => {
    const { formatAgo } = await import('./format')
    const now = Date.parse('2026-11-15T10:00:00')
    expect(formatAgo('2026-11-15T09:59:40', now)).toBe('zonet')
    expect(formatAgo('2026-11-15T09:53:00', now)).toBe('7 minuten geleden')
  })
  it('counts days back from today', async () => {
    const { daysAgoText } = await import('./format')
    expect(daysAgoText('2026-11-15', '2026-11-15')).toBe('vandaag')
    expect(daysAgoText('2026-11-14', '2026-11-15')).toBe('gisteren')
    expect(daysAgoText('2026-11-11', '2026-11-15')).toBe('4 dagen geleden')
  })
})
