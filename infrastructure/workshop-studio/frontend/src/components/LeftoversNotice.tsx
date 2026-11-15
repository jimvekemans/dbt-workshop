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
        <StatusMark state="ok" /> Nothing left over from earlier demos. Every scenario starts clean.
      </p>
    )
  }
  const from = left.map((l) => (l.from_scenario ? findCatalogScenario(catalog, l.from_scenario)?.title : null)).find(Boolean)
  return (
    <Callout
      tone="info"
      title="The last demo left its practice data behind"
      actions={
        <Button icon={<BroomIcon />} loading={cleaning} disabled={!!server.job && !cleaning} disabledReason="Wait until the current work has finished" onClick={() => void actions.cleanup()}>
          {cleaning ? 'Cleaning up…' : 'Clean up now'}
        </Button>
      }
    >
      {from && <p className={styles.from}>From: {from}</p>}
      <p>Harmless. Starting any scenario tidies this automatically.</p>
      <Disclosure summary="What exactly?">
        <ul className={styles.list}>
          {left.map((l, i) => (
            <li key={i}>{l.label}</li>
          ))}
        </ul>
      </Disclosure>
    </Callout>
  )
}
