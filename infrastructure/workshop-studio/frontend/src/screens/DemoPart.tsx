import { plainText } from '../lib/glossary'
import { useEffect, useRef, useState } from 'react'
import type { InputValue, InputValues, Scenario } from '../api/types'
import { useStudio } from '../lib/studio'
import { demoFor, demoIsOpen, stepBlockedReason, stepState } from '../lib/store'
import { defaultsFor } from '../lib/inputs'
import { Prose } from '../components/Term'
import { Button } from '../components/Button'
import { Callout } from '../components/Callout'
import { JumpNav } from '../components/JumpNav'
import { BroomIcon, RefreshIcon } from '../components/icons'
import { GettingReady } from './GettingReady'
import { StepCard, finalEvidence } from './StepCard'
import { partTitle } from '../lib/router'
import styles from './parts.module.css'
import demo from './DemoPart.module.css'

export function DemoPart({ scenario, focusStep }: { scenario: Scenario; focusStep?: string }) {
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

  // a link from a question names the step it is about: once the steps are on the page, go there (once). After the
  // page's own "start at the top" has run, so this is where the page ends up.
  const jumped = useRef<string | null>(null)
  useEffect(() => {
    if (!open || !focusStep || jumped.current === focusStep) return
    const timer = setTimeout(() => {
      const el = document.getElementById(`step-${focusStep}`)
      if (!el) return
      jumped.current = focusStep
      const card = el.closest('article') ?? el
      card.scrollIntoView?.({ behavior: 'auto', block: 'start' })
      el.focus?.({ preventScroll: true })
    }, 0)
    return () => clearTimeout(timer)
  }, [open, focusStep])

  return (
    <>
      <div className={styles.head}>
        <p className="kicker">{partTitle('demo')}</p>
        <h2 className={styles.h2} id="part-heading" tabIndex={-1}>
          Kijk nu hoe het gebeurt
        </h2>
        <Prose text={scenario.demo.intro} className={styles.lead} />
        {replay && (
          <Callout
            tone="warn"
            title="Dit is een opname"
            actions={
              <Button disabled={!!server?.job || !!server?.live_unavailable} disabledReason={server?.live_unavailable ?? 'Wacht tot het huidige werk klaar is'} onClick={() => void actions.setMode('live')}>
                Overschakelen naar Live
              </Button>
            }
          >
            {entry && !entry.recorded
              ? 'Van dit scenario bestaat er nog geen opname, dus de stappen kunnen niet draaien. Schakel over naar Live om het echt te runnen.'
              : "Het publiek bekijkt een opname van een eerdere run, sneller afgespeeld. Er wordt niets gewijzigd op het systeem. In Live draaien de echte stappen en duren ze zo lang als de tijden bij 'Meestal ongeveer'."}
          </Callout>
        )}
      </div>

      {!open && <GettingReady server={server} demo={d} scenarioTitle={plainText(scenario.title)} usualSeconds={scenario.prepare_usual_seconds} onPrepare={prepare} />}

      {open && steps.length > 4 && (
        <JumpNav
          label="De stappen van deze demo"
          items={steps.map((s, i) => {
            const status = stepState(d, s.id)?.status
            return {
              target: `step-${s.id}`,
              title: plainText(s.title),
              mark: status === 'done' ? '✓' : status === 'failed' ? '!' : String(i + 1),
              state: status === 'done' ? 'done' : status === 'failed' ? 'failed' : status === 'running' ? 'running' : 'todo',
            }
          })}
        />
      )}

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
            Dat was de hele demo
          </h3>
          <p className={demo.endText}>Je kan elke stap opnieuw runnen, deze demo opnieuw starten of een ander scenario kiezen.</p>
          {tryStep && !replay && (
            <p className={demo.endText}>
              Probeer het zelf: wijzig <b>{tryStep.inputs[0]?.label}</b> en druk op “Opnieuw runnen”.
            </p>
          )}
          <p className={demo.endText}>
            {left.length > 0
              ? `Deze demo wijzigde het oefensysteem een beetje (${left.map((l) => l.label).join('; ')}). Het wordt automatisch opgekuist als je een ander scenario start, of nu:`
              : replay
                ? 'Dit was een opname: er werd niets aangeraakt op het oefensysteem.'
                : 'Deze demo liet het oefensysteem zoals het was.'}
          </p>
          <div className={demo.endActions} data-end-actions="">
            {left.length > 0 && (
              <Button icon={<BroomIcon />} loading={cleaning} disabled={!!server?.job && !cleaning} disabledReason="Wacht tot het huidige werk klaar is" onClick={() => void actions.cleanup()}>
                {cleaning ? 'Alles wordt opgekuist…' : 'Alles opkuisen'}
              </Button>
            )}
            <Button icon={<RefreshIcon />} disabled={!!server?.job} disabledReason="Wacht tot het huidige werk klaar is" onClick={prepare}>
              Deze demo opnieuw starten
            </Button>
          </div>
          <p className={demo.endHint}>Opnieuw starten wist de resultaten op deze pagina.</p>
        </section>
      )}
    </>
  )
}
