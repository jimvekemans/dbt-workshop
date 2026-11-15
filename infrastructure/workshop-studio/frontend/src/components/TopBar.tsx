import { plainText } from '../lib/glossary'
import type { Route } from '../lib/router'
import { useStudio } from '../lib/studio'
import { useUi } from '../lib/ui'
import { neighbours } from '../lib/store'
import { ModeBadge } from './ModeBadge'
import { ThemeToggle } from './ThemeToggle'
import { TextSize } from './TextSize'
import { Button } from './Button'
import { ChevronLeftIcon, ChevronRightIcon, HomeIcon, SearchIcon } from './icons'
import styles from './TopBar.module.css'

/** One row that never changes height: whatever the studio is doing, nothing in here appears, wraps or moves. */
export function TopBar({ route }: { route: Route }) {
  const { catalog } = useStudio()
  const { openSwitcher } = useUi()
  const nb = route.name === 'scenario' ? neighbours(catalog, route.id) : null
  const here = nb && nb.index >= 0 ? nb : null
  const go = (id: string) => {
    window.location.hash = `#/s/${encodeURIComponent(id)}/problem`
  }
  return (
    <header className={styles.bar}>
      <div className={styles.inner}>
        <div className={styles.left}>
          <a className={styles.brand} href="#/" aria-label="Workshop Studio: back to all scenarios" title="Back to all scenarios">
            <HomeIcon size="1.2em" />
            <span>Workshop Studio</span>
          </a>
          <button type="button" className={styles.switcher} onClick={openSwitcher} aria-haspopup="dialog" aria-label="Scenarios: go to a scenario" aria-keyshortcuts="/ Control+K Meta+K">
            <SearchIcon size="1.1em" />
            <span className={styles.long}>Go to a scenario</span>
            <span className={styles.short}>Scenarios</span>
            <kbd className="hint-key" aria-hidden="true">/</kbd>
          </button>
        </div>
        <div className={styles.mid}>
          {here && (
            <>
              <Button
                variant="quiet"
                className={styles.nav}
                icon={<ChevronLeftIcon />}
                aria-label={here.prev ? `Previous scenario: ${plainText(here.prev.title)}` : 'Previous scenario'}
                disabled={!here.prev}
                disabledReason="This is the first scenario"
                onClick={() => here.prev && go(here.prev.id)}
              >
                <span className={styles.txt}>Previous scenario</span>
              </Button>
              <span className={styles.count}>
                <span className={styles.countLong}>Scenario {here.index + 1} of {here.total}</span>
                <span className={styles.countShort}>{here.index + 1}/{here.total}</span>
              </span>
              <Button
                variant="quiet"
                className={styles.nav}
                icon={<ChevronRightIcon />}
                aria-label={here.next ? `Next scenario: ${plainText(here.next.title)}` : 'Next scenario'}
                disabled={!here.next}
                disabledReason="This is the last scenario"
                onClick={() => here.next && go(here.next.id)}
              >
                <span className={styles.txt}>Next scenario</span>
              </Button>
            </>
          )}
        </div>
        <div className={styles.right}>
          <ModeBadge />
          <TextSize />
          <ThemeToggle />
        </div>
      </div>
    </header>
  )
}
