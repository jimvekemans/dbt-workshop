import { useStudio } from '../lib/studio'
import type { Mode } from '../api/types'
import { Button } from './Button'
import { ChoiceButtons } from './ChoiceButtons'
import { Dialog } from './Dialog'
import styles from './ModeDialog.module.css'

const OPTIONS = [
  { value: 'live', label: 'Live' },
  { value: 'replay', label: 'Recorded replay' },
]
const SENTENCE: Record<Mode, string> = {
  live: 'Every button does the real thing and you see the real result. A demo takes a few minutes.',
  replay: 'Plays back an earlier real run, faster. Use it when time is short or something is not working.',
}

/** The one place the mode is changed (opened from the header badge and from the status row). */
export function ModeDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { server, actions } = useStudio()
  const mode = server?.mode ?? 'live'
  const busy = !!server?.job
  return (
    <Dialog open={open} title="Change how the demos run" onClose={onClose} actions={<Button variant="primary" onClick={onClose}>Done</Button>}>
      <ChoiceButtons label="Mode" segments options={OPTIONS} value={mode} disabled={!server || busy} onChange={(v) => void actions.setMode(v as Mode)} disabledValues={server?.live_unavailable ? ['live'] : undefined} />
      <p className={styles.sentence}>{SENTENCE[mode]}</p>
      {server?.live_unavailable && (
        <p className={styles.hint} role="status">
          Live is not available right now. {server.live_unavailable}
        </p>
      )}
      {busy && <p className={styles.hint}>You can switch once the current work has finished.</p>}
    </Dialog>
  )
}
