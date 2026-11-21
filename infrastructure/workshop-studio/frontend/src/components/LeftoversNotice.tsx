import { useStudio } from '../lib/studio'
import { findCatalogScenario } from '../lib/store'
import { Callout } from './Callout'
import { Button } from './Button'
import { Disclosure } from './Disclosure'
import { BroomIcon } from './icons'
import { StatusMark } from './Status'
import styles from './LeftoversNotice.module.css'

/** What an earlier demo left behind. Harmless, so it is neutral, and cleaning up asks for no confirmation. */
export function LeftoversNotice() {
  const { server, catalog, actions } = useStudio()
  if (!server) return null
  const left = server.leftovers
  const cleaning = server.job?.kind === 'cleanup'
  if (left.length === 0) {
    return (
      <p className={styles.tidy}>
        <StatusMark state="ok" /> Er blijft niets over van vorige demo's. Elk scenario start schoon.
      </p>
    )
  }
  const from = left.map((l) => (l.from_scenario ? findCatalogScenario(catalog, l.from_scenario)?.title : null)).find(Boolean)
  return (
    <Callout
      tone="info"
      title="De laatste demo liet oefendata achter"
      actions={
        <Button icon={<BroomIcon />} loading={cleaning} disabled={!!server.job && !cleaning} disabledReason="Wacht tot het huidige werk klaar is" onClick={() => void actions.cleanup()}>
          {cleaning ? 'Bezig met opkuisen…' : 'Nu opkuisen'}
        </Button>
      }
    >
      {from && <p className={styles.from}>Van: {from}</p>}
      <p>Onschuldig. Elk scenario dat je start, kuist dit automatisch op.</p>
      <Disclosure summary="Wat precies?">
        <ul className={styles.list}>
          {left.map((l, i) => (
            <li key={i}>{l.label}</li>
          ))}
        </ul>
      </Disclosure>
    </Callout>
  )
}
