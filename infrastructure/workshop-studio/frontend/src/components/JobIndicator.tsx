import { plainText } from '../lib/glossary'
import { useState } from 'react'
import { jobStartedMs } from '../api/adapt'
import type { Job, Scenario } from '../api/types'
import { useStudio } from '../lib/studio'
import { useNow } from '../lib/useNow'
import { elapsedSeconds, formatDuration, usualText } from '../lib/format'
import { cx } from '../lib/cx'
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
      text: i >= 0 ? `Stap ${i + 1} loopt: ${plainText(def?.title ?? job.label)}` : `Bezig: ${plainText(job.label)}`,
      usual: usualText(def?.usual_seconds),
      stop: 'Stop deze stap',
      confirm: 'Deze stap stoppen?',
      yes: 'Stop de stap',
      body: `De stap “${plainText(def?.title ?? job.label)}” stopt nu. Wat ze al wijzigde in de oefendata, wordt automatisch opgekuist als je de volgende stap of het volgende scenario start. Je kan deze stap opnieuw runnen.`,
    }
  }
  if (job.kind === 'prepare') {
    return {
      text: job.scenario ? 'De demo wordt klaargezet' : 'Het oefensysteem wordt klaargezet',
      usual: usualText(sc?.prepare_usual_seconds),
      stop: 'Stop het klaarzetten',
      confirm: 'Het klaarzetten stoppen?',
      yes: 'Stop het klaarzetten',
      body: 'Het klaarzetten stopt nu. Wat het publiek al zag, gaat niet verloren. Je kan het opnieuw starten met “Demo klaarzetten”.',
    }
  }
  return {
    text: 'Alles wordt opgekuist',
    usual: null,
    stop: 'Stop het opkuisen',
    confirm: 'Het opkuisen stoppen?',
    yes: 'Stop het opkuisen',
    body: 'Het opkuisen stopt waar het nu is. Elk scenario dat je start, maakt het voor je af.',
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
  const stoppable = job.cancellable && server?.mode !== 'replay'
  return (
    <>
      <div className={cx(styles.job, stoppable && styles.withStop)} role="status" data-stoppable={stoppable ? '' : undefined}>
        <StatusMark state="running" size="1.3em" />
        <span className={styles.label}>
          {d.text}
          {d.usual && <span className={styles.usual}> ({d.usual})</span>}
        </span>
        <span className={styles.time} aria-hidden="true">
          · {formatDuration(elapsedSeconds(jobStartedMs(job), now))}
        </span>
        {stoppable && (
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
              Ga verder
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
