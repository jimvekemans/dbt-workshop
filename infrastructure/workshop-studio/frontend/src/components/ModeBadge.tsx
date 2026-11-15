import { useStudio } from '../lib/studio'
import { useUi } from '../lib/ui'
import { cx } from '../lib/cx'
import styles from './ModeBadge.module.css'

/** Always visible: is the audience watching the real thing or a recording? Pressing it opens the mode control. */
export function ModeBadge() {
  const { server, connected } = useStudio()
  const { openMode } = useUi()
  if (!connected) return <span className={cx(styles.badge, styles.wait)}>Offline</span>
  if (!server) return <span className={cx(styles.badge, styles.wait)}>Connecting…</span>
  const live = server.mode === 'live'
  return (
    <button
      type="button"
      className={cx(styles.badge, styles.button, live ? styles.live : styles.replay)}
      title="Change how the demos run"
      aria-label={live ? 'Live. Change how the demos run' : 'Recorded replay. Change how the demos run'}
      onClick={openMode}
    >
      <span className={styles.dot} aria-hidden="true">
        {live ? '●' : '▶'}
      </span>
      {live ? (
        'Live'
      ) : (
        <>
          <span className={styles.long}>Recorded replay</span>
          <span className={styles.short}>Replay</span>
        </>
      )}
    </button>
  )
}
