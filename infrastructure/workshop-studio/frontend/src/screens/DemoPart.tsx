import { plainText } from '../lib/glossary'
import { useState } from 'react'
import type { InputValue, InputValues, Scenario } from '../api/types'
import { useStudio } from '../lib/studio'
import { demoFor, demoIsOpen, stepBlockedReason, stepState } from '../lib/store'
import { defaultsFor } from '../lib/inputs'
import { Prose } from '../components/Term'
import { Button } from '../components/Button'
import { Callout } from '../components/Callout'
import { BroomIcon, RefreshIcon } from '../components/icons'
import { GettingReady } from './GettingReady'
import { StepCard, finalEvidence } from './StepCard'
import { partTitle } from '../lib/router'
import styles from './parts.module.css'
import demo from './DemoPart.module.css'

export function DemoPart({ scenario }: { scenario: Scenario }) {
  const { server, catalog, actions } = useStudio()
  const d = demoFor(server, scenario.id)
  const open = demoIsOpen(d)
  const replay = server?.mode === 'replay'
  const entry = catalog?.groups.flatMap((g) => g.scenarios).find((s) => s.id === scenario.id) ?? null
  const [values, setValues] = useState<Record<string, InputValues>>(() =>
    Object.fromEntries(scenario.demo.steps.map((s) => [s.id, defaultsFor(s.inputs)])),
  )

  // Nothing is prepared by opening this part: preparing changes real systems and can take minutes, so it is a button.
  const prepare = () => void actions.prepare(scenario.id)

  const setValue = (stepId: string, id: string, v: InputValue) =>
    setValues((all) => ({ ...all, [stepId]: { ...(all[stepId] ?? {}), [id]: v } }))

  const steps = scenario.demo.steps
  const allDone = open && steps.every((s) => stepState(d, s.id)?.status === 'done')
  const tryStep = [...steps].reverse().find((s) => s.inputs.length > 0)
  const left = server?.leftovers ?? []
  const cleaning = server?.job?.kind === 'cleanup'

  return (
    <>
      <div className={styles.head}>
        <p className="kicker">{partTitle('demo')}</p>
        <h2 className={styles.h2} id="part-heading" tabIndex={-1}>
          Now watch it happen
        </h2>
        <Prose text={scenario.demo.intro} className={styles.lead} />
        {replay && (
          <Callout
            tone="warn"
            title="This is a replay"
            actions={
              <Button disabled={!!server?.job || !!server?.live_unavailable} disabledReason={server?.live_unavailable ?? 'Wait until the current work has finished'} onClick={() => void actions.setMode('live')}>
                Switch to Live
              </Button>
            }
          >
            {entry && !entry.recorded
              ? 'There is no recording of this scenario yet, so the steps cannot run. Switch to Live to run it for real.'
              : "The audience is watching a recording from an earlier run, played back faster. Nothing on the system is changed. In Live the real steps run and take as long as the 'Usually about' times."}
          </Callout>
        )}
      </div>

      {!open && <GettingReady server={server} demo={d} scenarioTitle={plainText(scenario.title)} usualSeconds={scenario.prepare_usual_seconds} onPrepare={prepare} />}

      {open && (
        <ol className={demo.steps}>
          {steps.map((def, i) => (
            <li key={def.id} className={demo.step}>
              <StepCard
                scenarioId={scenario.id}
                index={i}
                total={steps.length}
                def={def}
                state={stepState(d, def.id)}
                blocked={stepBlockedReason(server, scenario, entry, i)}
                values={replay ? defaultsFor(def.inputs) : (values[def.id] ?? {})}
                onValue={(id, v) => setValue(def.id, id, v)}
                onRun={() => void actions.runStep(scenario.id, def.id, replay ? defaultsFor(def.inputs) : (values[def.id] ?? {}))}
                replay={replay}
                beforeFor={(stepId, type) => finalEvidence(stepState(d, stepId), type)}
              />
            </li>
          ))}
        </ol>
      )}

      {allDone && (
        <section className={demo.end} aria-labelledby="end-h" data-end-card="">
          <h3 id="end-h" className={demo.endTitle}>
            That is the whole demo
          </h3>
          <p className={demo.endText}>You can run any step again, start this demo over, or pick another scenario.</p>
          {tryStep && !replay && (
            <p className={demo.endText}>
              Try it yourself: change the <b>{tryStep.inputs[0]?.label}</b> and press “Run again”.
            </p>
          )}
          <p className={demo.endText}>
            {left.length > 0
              ? `This demo changed the practice system a little (${left.map((l) => l.label).join('; ')}). It is put back automatically when you start another scenario, or now:`
              : replay
                ? 'This was a recording: nothing on the practice system was touched.'
                : 'This demo left the practice system as it was.'}
          </p>
          <div className={demo.endActions} data-end-actions="">
            {left.length > 0 && (
              <Button icon={<BroomIcon />} loading={cleaning} disabled={!!server?.job && !cleaning} disabledReason="Wait until the current work has finished" onClick={() => void actions.cleanup()}>
                {cleaning ? 'Putting everything back…' : 'Put everything back'}
              </Button>
            )}
            <Button icon={<RefreshIcon />} disabled={!!server?.job} disabledReason="Wait until the current work has finished" onClick={prepare}>
              Start this demo over
            </Button>
          </div>
          <p className={demo.endHint}>Starting over clears the results on this page.</p>
        </section>
      )}
    </>
  )
}
