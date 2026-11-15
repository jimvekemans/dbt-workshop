import { describe, expect, it } from 'vitest'
import { lookupTerm, parseGlossary, stripTerms } from './glossary'

describe('glossary parser', () => {
  it('returns plain text untouched', () => {
    expect(parseGlossary('nothing here')).toEqual([{ kind: 'text', text: 'nothing here' }])
  })
  it('splits terms out of a sentence', () => {
    expect(parseGlossary('We rely on a [[feed]] every day.')).toEqual([
      { kind: 'text', text: 'We rely on a ' },
      { kind: 'term', term: 'feed', label: 'feed' },
      { kind: 'text', text: ' every day.' },
    ])
  })
  it('supports a different shown label and several terms', () => {
    const segs = parseGlossary('[[nightly load|the night]] and [[release]]')
    expect(segs).toHaveLength(3)
    expect(segs[0]).toEqual({ kind: 'term', term: 'nightly load', label: 'the night' })
    expect(segs[2]).toEqual({ kind: 'term', term: 'release', label: 'release' })
  })
  it('leaves unfinished markers alone', () => {
    expect(parseGlossary('a [[broken')).toEqual([{ kind: 'text', text: 'a [[broken' }])
  })
  it('looks terms up case-insensitively and strips markup', () => {
    expect(lookupTerm({ Feed: 'x' }, 'feed')).toBe('x')
    expect(lookupTerm({ Feed: 'x' }, 'nope')).toBeUndefined()
    expect(stripTerms('a [[feed]] b')).toBe('a feed b')
  })
})
