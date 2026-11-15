import { plainText } from '../lib/glossary'
import { useState } from 'react'
import { jobStartedMs } from '../api/adapt'
import type { Job, Scenario } from '../api/types'
import { useStudio } from '../lib/studio'
import { useNow } from '../lib/useNow'
import { elapsedSeconds, formatDuration, usualText } from '../lib/format'
import { Button } from './Button'
import { Dialog } from './Dialog'
import { StopIcon } from './icons'
import { StatusMark } from './Status'
import styles from './JobIndicator.module.css'

/** What the pill says, what its stop button says, and what the confirmation asks. */
export function describeJob(job: Job, scenarios: Record<string, Scenario>) {
  const sc = job.scenario ? scenarios[job.scenario] : undefined
  if (job.kind === 'step') {
    const i = sc?.demo.steps.findIndex((s) => s.id === job.step) ?? -1
    const def = i >= 0 ? sc?.demo.steps[i] : undefined
    return {
      text: i >= 0 ? `Step ${i + 1} is running: ${plainText(def?.title ?? job.label)}` : `Running: ${plainText(job.label)}`,
      usual: usualText(def?.usual_seconds),
      stop: 'Stop this step',
      confirm: 'Stop this step?',
      yes: 'Stop the step',
      body: `The step “${plainText(def?.title ?? job.label)}” will stop now. Anything it already changed in the practice data is tidied automatically when you start the next step or scenario. You can run this step again.`,
    }
  }
  if (job.kind === 'prepare') {
    return {
      text: job.scenario ? 'Getting the demo ready' : 'Getting the practice system ready',
      usual: usualText(sc?.prepare_usual_seconds),
      stop: 'Stop getting ready',
      confirm: 'Stop getting ready?',
      yes: 'Stop getting ready',
      body: 'Getting ready will stop now. Nothing the audience has seen is lost. You can start it again with “Prepare this demo”.',
    }
  }
  return {
    text: 'Putting everything back',
    usual: null,
    stop: 'Stop putting back',
    confirm: 'Stop putting everything back?',
    yes: 'Stop putting back',
    body: 'The clean up will stop where it is. Starting any scenario finishes it for you.',
  }
}

/** A fixed pill at the bottom centre: always visible while the studio is busy, and nothing on the page moves for it. */
export function JobIndicator() {
  const { server, scenarios, actions } = useStudio()
  const [confirm, setConfirm] = useState(false)
  const job = server?.job ?? null
  const now = useNow(!!job)
  if (!job) return null
  const d = describeJob(job, scenarios)
  return (
    <>
      <div className={styles.job} role="status">
        <StatusMark state="running" size="1.3em" />
        <span className={styles.label}>
          {d.text}
          {d.usual && <span className={styles.usual}> ({d.usual})</span>}
        </span>
        <span className={styles.time} aria-hidden="true">
          · {formatDuration(elapsedSeconds(jobStartedMs(job), now))}
        </span>
        {job.cancellable && server?.mode !== 'replay' && (
          <Button variant="danger" icon={<StopIcon size="0.8em" />} onClick={() => setConfirm(true)}>
            {d.stop}
          </Button>
        )}
      </div>
      <Dialog
        open={confirm}
        title={d.confirm}
        onClose={() => setConfirm(false)}
        actions={
          <>
            <Button variant="primary" onClick={() => setConfirm(false)}>
              Keep going
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                setConfirm(false)
                void actions.cancel()
              }}
            >
              {d.yes}
            </Button>
          </>
        }
      >
        <p>{d.body}</p>
      </Dialog>
    </>
  )
}
