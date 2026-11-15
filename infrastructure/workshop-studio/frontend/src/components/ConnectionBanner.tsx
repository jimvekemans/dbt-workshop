import { useEffect, useRef, useState } from 'react'
import { useStudio } from '../lib/studio'
import { Callout } from './Callout'
import { Button } from './Button'
import styles from './ConnectionBanner.module.css'

const RETRY_SECONDS = 5

export function ConnectionBanner() {
  const { connected, actions } = useStudio()
  const [left, setLeft] = useState(RETRY_SECONDS)
  const [back, setBack] = useState(false)
  const was = useRef(connected)

  // a green "Back online" for 3 seconds after the connection returns
  useEffect(() => {
    const returned = connected && !was.current
    was.current = connected
    if (!returned) return
    setBack(true)
    const t = setTimeout(() => setBack(false), 3000)
    return () => clearTimeout(t)
  }, [connected])

  // while offline: count down and ask again by ourselves (the live connection also retries on its own)
  useEffect(() => {
    if (connected) {
      setLeft(RETRY_SECONDS)
      return
    }
    const t = setTimeout(() => {
      if (left <= 1) {
        actions.retryConnection()
        setLeft(RETRY_SECONDS)
      } else setLeft(left - 1)
    }, 1000)
    return () => clearTimeout(t)
  }, [connected, left, actions])

  if (connected && !back) return null
  return (
    <div className={styles.banner}>
      {!connected && (
        <Callout
          tone="bad"
          role="alert"
          title="The demo system cannot be reached"
          actions={
            <>
              <span aria-hidden="true">Trying again in {left} s</span>
              <Button onClick={actions.retryConnection}>Try now</Button>
            </>
          }
        >
          We are trying again every few seconds. What you see may be out of date.
        </Callout>
      )}
      {connected && back && <Callout tone="good" role="status" title="Back online" />}
    </div>
  )
}
