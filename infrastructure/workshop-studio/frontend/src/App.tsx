import { useEffect, useMemo, useState } from 'react'
import { useRoute, PARTS, navigate } from './lib/router'
import { useStudio } from './lib/studio'
import { TermProvider } from './components/Term'
import { TopBar } from './components/TopBar'
import { NoticeBar } from './components/NoticeBar'
import { ConnectionBanner } from './components/ConnectionBanner'
import { Switcher } from './components/Switcher'
import { ModeDialog } from './components/ModeDialog'
import { JobIndicator } from './components/JobIndicator'
import { Callout } from './components/Callout'
import { Button } from './components/Button'
import { UiContext } from './lib/ui'
import { Home } from './screens/Home'
import { ScenarioPage } from './screens/ScenarioPage'
import { Roadmap } from './screens/Roadmap'

function typingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false
  return t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName)
}

export function App() {
  const route = useRoute()
  const { catalog } = useStudio()
  const [switcher, setSwitcher] = useState(false)
  const [modeOpen, setModeOpen] = useState(false)
  const [lostPage, setLostPage] = useState(false)
  const ui = useMemo(() => ({ openMode: () => setModeOpen(true), openSwitcher: () => setSwitcher(true) }), [])

  // a link that is no page of this app: go to the start page and say so; a scenario link with a missing or wrong part: use part 1
  useEffect(() => {
    if (route.name === 'unknown') {
      setLostPage(true)
      window.location.replace('#/')
    } else if (route.name === 'scenario' && route.repair) window.location.replace(`#/s/${encodeURIComponent(route.id)}/problem`)
    else if (route.name === 'scenario') setLostPage(false)
  }, [route])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault()
        setSwitcher((v) => !v)
        return
      }
      if (e.ctrlKey || e.metaKey || e.altKey) return
      if (document.querySelector('[role="dialog"][aria-modal="true"]')) return
      if (typingTarget(e.target)) return
      if (e.key === '/') {
        e.preventDefault()
        setSwitcher(true)
        return
      }
      if (route.name === 'scenario' && (e.key === 'ArrowRight' || e.key === 'ArrowLeft') && !e.shiftKey) {
        const i = PARTS.findIndex((p) => p.id === route.part)
        const target = PARTS[i + (e.key === 'ArrowRight' ? 1 : -1)]
        if (target) {
          e.preventDefault()
          navigate({ name: 'scenario', id: route.id, part: target.id })
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [route])

  return (
    <UiContext.Provider value={ui}>
    <TermProvider glossary={catalog?.glossary ?? {}}>
      <a className="skip-link" href="#main">
        Ga naar de inhoud
      </a>
      <div className="chrome">
        <TopBar route={route} />
        <ConnectionBanner />
      </div>
      <main id="main" tabIndex={-1} className="page">
        <NoticeBar />
        {lostPage && route.name !== 'scenario' && (
          <div style={{ marginBottom: 'var(--space-5)' }}>
            <Callout tone="info" role="status" actions={<Button variant="quiet" onClick={() => setLostPage(false)}>Sluiten</Button>}>
              Die pagina bestaat niet, daarom zie je de startpagina.
            </Callout>
          </div>
        )}
        {route.name === 'scenario' ? <ScenarioPage key={route.id} id={route.id} part={route.part} from={route.from} /> : route.name === 'roadmap' ? <Roadmap /> : <Home />}
      </main>
      <JobIndicator />
      <Switcher open={switcher} onClose={() => setSwitcher(false)} currentId={route.name === 'scenario' ? route.id : null} />
      <ModeDialog open={modeOpen} onClose={() => setModeOpen(false)} />
    </TermProvider>
    </UiContext.Provider>
  )
}
