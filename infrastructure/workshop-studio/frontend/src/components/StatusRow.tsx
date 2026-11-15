import { useEffect, useId, useRef, useState } from 'react'
import { useStudio } from '../lib/studio'
import { useUi } from '../lib/ui'
import { health } from '../lib/health'
import { Button } from './Button'
import { Skeleton } from './Skeleton'
import { StatusMark } from './Status'
import { ChevronDownIcon } from './icons'
import { StatusDetails } from './StatusDetails'
import { cx } from '../lib/cx'
import styles from './StatusRow.module.css'

/** ONE quiet row under the intro: how the system is doing (with Details) and how the demos run (with Change). */
export function StatusRow() {
  const { server, connected } = useStudio()
  const { openMode } = useUi()
  const h = server ? health(server) : null
  const attention = !!h?.attention && connected
  const [open, setOpen] = useState(attention)
  const was = useRef(attention)
  const touched = useRef(false)
  const panel = useId()

  // Details opens by itself when something needs a look, and closes again when that is over (unless the presenter chose).
  useEffect(() => {
    if (attention && !was.current) {
      touched.current = false
      setOpen(true)
    } else if (!attention && was.current && !touched.current) setOpen(false)
    was.current = attention
  }, [attention])

  const mode = server?.mode === 'replay' ? 'Demos play from a recording.' : 'Demos run live.'
  return (
    <section className={styles.wrap} aria-label="System status and mode">
      <div className={styles.row}>
        <div className={styles.left}>
          {!connected ? (
            <p className={styles.line}>
              <StatusMark state="idle" /> Status unknown while offline
            </p>
          ) : !h ? (
            <Skeleton width="16rem" height="1.2em" />
          ) : (
            <div className={styles.lines}>
              {h.headline && (
                <p className={cx(styles.line, h.warn && styles.warn)}>
                  <StatusMark state={h.warn ? 'warn' : 'ok'} /> {h.headline}
                </p>
              )}
              {h.baselineLine && (
                <p className={styles.line}>
                  <StatusMark state={server?.waiting ? 'running' : server?.baseline.status === 'failed' ? 'bad' : server?.baseline.status === 'preparing' ? 'running' : 'idle'} /> {h.baselineLine}
                </p>
              )}
            </div>
          )}
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={open}
            aria-controls={panel}
            onClick={() => {
              touched.current = true
              setOpen((v) => !v)
            }}
          >
            Details <ChevronDownIcon className={cx(styles.chev, open && styles.chevOpen)} />
          </button>
        </div>
        <div className={styles.right}>
          <span className={styles.mode}>
            <b>{server ? mode : '…'}</b>
          </span>
          <Button variant="quiet" onClick={openMode} disabled={!server}>
            Change demo mode
          </Button>
        </div>
      </div>
      {open && (
        <div id={panel} className={styles.details}>
          <StatusDetails />
        </div>
      )}
    </section>
  )
}
