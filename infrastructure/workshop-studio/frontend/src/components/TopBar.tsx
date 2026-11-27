import { plainText } from '../lib/glossary'
import type { Route } from '../lib/router'
import { useStudio } from '../lib/studio'
import { useUi } from '../lib/ui'
import { neighbours } from '../lib/store'
import { quizItems } from '../lib/quiz'
import { ModeBadge } from './ModeBadge'
import { ThemeToggle } from './ThemeToggle'
import { TextSize } from './TextSize'
import { TechNamesToggle } from './TechNamesToggle'
import { Button } from './Button'
import { ChevronLeftIcon, ChevronRightIcon, HomeIcon, SearchIcon } from './icons'
import styles from './TopBar.module.css'

/** One row that never changes height: whatever the studio is doing, nothing in here appears, wraps or moves. */
export function TopBar({ route }: { route: Route }) {
  const { catalog } = useStudio()
  const { openSwitcher } = useUi()
  const nb = route.name === 'scenario' ? neighbours(catalog, route.id) : null
  const here = nb && nb.index >= 0 ? nb : null
  const hasQuiz = quizItems(catalog).length > 0
  const inQuiz = route.name === 'quiz' || route.name === 'question'
  const go = (id: string) => {
    window.location.hash = `#/s/${encodeURIComponent(id)}/problem`
  }
  return (
    <header className={styles.bar}>
      <div className={styles.inner}>
        <div className={styles.left}>
          <a className={styles.brand} href="#/" aria-label="Workshop Studio: terug naar alle scenario's" title="Terug naar alle scenario's">
            <HomeIcon size="1.2em" />
            <span>Workshop Studio</span>
          </a>
          <button type="button" className={styles.switcher} onClick={openSwitcher} aria-haspopup="dialog" aria-label="Scenario's: ga naar een scenario" aria-keyshortcuts="/ Control+K Meta+K">
            <SearchIcon size="1.1em" />
            <span className={styles.long}>Ga naar een scenario</span>
            <span className={styles.short}>Scenario's</span>
            <kbd className="hint-key" aria-hidden="true">/</kbd>
          </button>
          {hasQuiz && (
            <a className={styles.quiz} href="#/quiz" aria-current={inQuiz ? 'page' : undefined} title="De quiz: alle vragen aan de zaal">
              Quiz
            </a>
          )}
        </div>
        <div className={styles.mid}>
          {here && (
            <>
              <Button
                variant="quiet"
                className={styles.nav}
                icon={<ChevronLeftIcon />}
                aria-label={here.prev ? `Vorig scenario: ${plainText(here.prev.title)}` : 'Vorig scenario'}
                disabled={!here.prev}
                disabledReason="Dit is het eerste scenario"
                onClick={() => here.prev && go(here.prev.id)}
              >
                <span className={styles.txt}>Vorig scenario</span>
              </Button>
              <span className={styles.count}>
                <span className={styles.countLong}>Scenario {here.index + 1} van {here.total}</span>
                <span className={styles.countShort}>{here.index + 1}/{here.total}</span>
              </span>
              <Button
                variant="quiet"
                className={styles.nav}
                icon={<ChevronRightIcon />}
                aria-label={here.next ? `Volgend scenario: ${plainText(here.next.title)}` : 'Volgend scenario'}
                disabled={!here.next}
                disabledReason="Dit is het laatste scenario"
                onClick={() => here.next && go(here.next.id)}
              >
                <span className={styles.txt}>Volgend scenario</span>
              </Button>
            </>
          )}
        </div>
        <div className={styles.right}>
          <ModeBadge />
          <TechNamesToggle />
          <TextSize />
          <ThemeToggle />
        </div>
      </div>
    </header>
  )
}
