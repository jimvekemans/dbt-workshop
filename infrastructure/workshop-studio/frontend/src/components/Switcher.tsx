import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { plainText } from '../lib/glossary'
import { Dialog } from './Dialog'
import { useStudio } from '../lib/studio'
import { searchScenarios } from '../lib/store'
import { navigate } from '../lib/router'
import { cx } from '../lib/cx'
import { Button } from './Button'
import styles from './Switcher.module.css'

/** Searchable, grouped scenario list. Type to filter, arrows to move, Enter to go, Esc to close. */
export function Switcher({ open, onClose, currentId }: { open: boolean; onClose: () => void; currentId: string | null }) {
  const { catalog } = useStudio()
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const listRef = useRef<HTMLDivElement>(null)
  const uid = useId()

  useEffect(() => {
    if (open) {
      setQuery('')
      setActive(0)
    }
  }, [open])

  const results = useMemo(() => searchScenarios(catalog, query), [catalog, query])
  const total = useMemo(() => searchScenarios(catalog, '').length, [catalog])
  useEffect(() => setActive(0), [query])
  useEffect(() => {
    listRef.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [active])

  const pick = (i: number) => {
    const s = results[i]
    if (!s) return
    onClose()
    navigate({ name: 'scenario', id: s.id, part: 'problem' })
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setActive((a) => Math.min(results.length - 1, a + 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setActive((a) => Math.max(0, a - 1))
    } else if (e.key === 'Home' && e.ctrlKey) {
      setActive(0)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      pick(active)
    }
  }

  let lastGroup = ''
  return (
    <Dialog open={open} title={total > 0 ? `Go to a scenario (${total})` : 'Go to a scenario'} onClose={onClose} wide>
      <div className={styles.search}>
        <label htmlFor={`${uid}-q`} className="sr-only">
          Search the scenarios
        </label>
        <input
          id={`${uid}-q`}
          className={styles.input}
          type="search"
          autoComplete="off"
          placeholder="Search, for example “mail” or “release”"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          role="combobox"
          aria-expanded={results.length > 0}
          aria-controls={results.length > 0 ? `${uid}-list` : undefined}
          aria-activedescendant={results[active] ? `${uid}-o-${active}` : undefined}
          aria-autocomplete="list"
        />
      </div>
      <div className={styles.none} aria-live="polite">
        {results.length === 0 && (
          <>
            <p>
              Nothing matches “{query}”. Clear the search to see all {total}.
            </p>
            <p>Try a word from the title, like “mail” or “release”.</p>
            <Button onClick={() => setQuery('')}>Clear search</Button>
          </>
        )}
      </div>
      {results.length > 0 && (
        <div className={styles.listWrap}>
          <div id={`${uid}-list`} ref={listRef} className={styles.list} role="listbox" aria-label="Scenarios">
            {results.map((s, i) => {
              const header = s.groupTitle !== lastGroup ? s.groupTitle : null
              lastGroup = s.groupTitle
              return (
                <div key={s.id} role="presentation">
                  {header && <p className={styles.group} role="presentation">{header}</p>}
                  <div
                    id={`${uid}-o-${i}`}
                    data-index={i}
                    role="option"
                    aria-selected={i === active}
                    className={cx(styles.option, i === active && styles.active)}
                    onMouseMove={() => i !== active && setActive(i)}
                    onClick={() => pick(i)}
                  >
                    <span className={styles.title}>{plainText(s.title)}</span>
                    <span className={styles.meta}>
                      {s.id === currentId ? '(you are here) · ' : ''}
                      {s.minutes} min
                    </span>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      )}
      {results.length > 0 && (
        <p className={styles.hint} aria-hidden="true">
          <kbd>↑</kbd> <kbd>↓</kbd> to move, <kbd>Enter</kbd> to open, <kbd>Esc</kbd> to close. Press <kbd>/</kbd> or <kbd>Ctrl</kbd>+<kbd>K</kbd> to open this anywhere.
        </p>
      )}
      <p className={styles.home}>
        <a href="#/" onClick={onClose}>
          Back to the start page
        </a>
      </p>
    </Dialog>
  )
}
