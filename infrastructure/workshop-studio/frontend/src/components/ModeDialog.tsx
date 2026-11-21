import { useStudio } from '../lib/studio'
import type { Mode } from '../api/types'
import { Button } from './Button'
import { ChoiceButtons } from './ChoiceButtons'
import { Dialog } from './Dialog'
import styles from './ModeDialog.module.css'

const OPTIONS = [
  { value: 'live', label: 'Live' },
  { value: 'replay', label: 'Opname' },
]
const SENTENCE: Record<Mode, string> = {
  live: 'Elke knop doet het echte werk en je ziet het echte resultaat. Een demo duurt een paar minuten.',
  replay: 'Speelt een eerdere echte run sneller af. Gebruik dit als de tijd krap is of er iets niet werkt.',
}

/** The one place the mode is changed (opened from the header badge and from the status row). */
export function ModeDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { server, actions } = useStudio()
  const mode = server?.mode ?? 'live'
  const busy = !!server?.job
  return (
    <Dialog open={open} title="Wijzig hoe de demo's draaien" onClose={onClose} actions={<Button variant="primary" onClick={onClose}>Klaar</Button>}>
      <ChoiceButtons label="Modus" segments options={OPTIONS} value={mode} disabled={!server || busy} onChange={(v) => void actions.setMode(v as Mode)} disabledValues={server?.live_unavailable ? ['live'] : undefined} />
      <p className={styles.sentence}>{SENTENCE[mode]}</p>
      {server?.live_unavailable && (
        <p className={styles.hint} role="status">
          Live is nu niet beschikbaar. {server.live_unavailable}
        </p>
      )}
      {busy && <p className={styles.hint}>Je kan wisselen zodra het huidige werk klaar is.</p>}
    </Dialog>
  )
}
