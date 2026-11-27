import { useEffect, useMemo } from 'react'
import { useStudio } from '../lib/studio'
import { buildRoadmap } from '../lib/roadmap'
import { quizItems, score, useQuiz } from '../lib/quiz'
import type { RoadmapRow, RoadmapSection } from '../lib/roadmap'
import { plainText } from '../lib/glossary'
import { RichText } from '../components/Term'
import { TechLine } from '../components/TechLine'
import { LinkButton } from '../components/LinkButton'
import { Button } from '../components/Button'
import { Callout } from '../components/Callout'
import { SkeletonLines } from '../components/Skeleton'
import { ChevronLeftIcon } from '../components/icons'
import styles from './Roadmap.module.css'

function Row({ row }: { row: RoadmapRow }) {
  return (
    <li className={styles.row}>
      <span className={styles.what}>
        {row.scenarioId ? (
          <a className={styles.link} href={`#/s/${encodeURIComponent(row.scenarioId)}/${row.part ?? 'problem'}`}>
            {plainText(row.feature)}
          </a>
        ) : (
          <span className={styles.name}><RichText text={row.feature} /></span>
        )}
        <TechLine as="span" tech={row.tech} />
        {row.why && (
          <span className={styles.why}>
            <span className={styles.whyLabel}>Zonder: </span>
            <RichText text={row.why} />
          </span>
        )}
        {row.note && <span className={styles.note}><RichText text={row.note} /></span>}
      </span>
    </li>
  )
}

function Section({ section }: { section: RoadmapSection }) {
  const today = section.status === 'today'
  const id = `rm-${section.status}`
  return (
    <section className={styles.group} data-status={section.status} aria-labelledby={id}>
      <h2 id={id} className={styles.sectionTitle}>
        {today ? 'Vandaag al in gebruik' : 'Volgende stappen'} <span className={styles.sectionCount}>({section.count})</span>
      </h2>
      {section.count === 0 && <p className={styles.none}>{today ? 'Er is nog niets gemarkeerd als in gebruik.' : 'Er zijn geen volgende stappen gemarkeerd.'}</p>}
      {section.groups.map((g) => (
        <div key={g.id} className={styles.groupBlock}>
          <h3 className={styles.groupTitle}>
            <RichText text={g.title} />
          </h3>
          {g.blocks.map((b) => (
            <div key={b.key} className={styles.block}>
              {b.title && b.scenarioId && (
                <h4 className={styles.blockTitle}>
                  <a className={styles.link} href={`#/s/${encodeURIComponent(b.scenarioId)}/problem`}>
                    {plainText(b.title)}
                  </a>
                </h4>
              )}
              <ul className={styles.rows}>
                {b.rows.map((r) => (
                  <Row key={r.key} row={r} />
                ))}
              </ul>
            </div>
          ))}
        </div>
      ))}
      {section.areas.map((a) => (
        <div key={a.area} className={styles.groupBlock}>
          <h3 className={styles.groupTitle}>{plainText(a.area)}</h3>
          <ul className={styles.rows}>
            {a.rows.map((r) => (
              <Row key={r.key} row={r} />
            ))}
          </ul>
        </div>
      ))}
    </section>
  )
}

/** The page that ends the workshop: what is in place today and what the next steps are. */
export function Roadmap() {
  const { catalog, catalogError, connected, actions } = useStudio()
  const roadmap = catalog?.roadmap ?? null
  const view = useMemo(() => buildRoadmap(catalog), [catalog])
  const quiz = useQuiz()
  const room = useMemo(() => score(quizItems(catalog), quiz), [catalog, quiz])

  useEffect(() => {
    document.title = roadmap ? `${plainText(roadmap.title)} · Workshop Studio` : 'Workshop Studio'
    return () => {
      document.title = 'Workshop Studio'
    }
  }, [roadmap])

  // a new page starts at the top, with the heading focused (as the parts of a scenario do)
  const ready = !!roadmap
  useEffect(() => {
    if (!ready) return
    window.scrollTo?.({ top: 0 })
    document.getElementById('part-heading')?.focus({ preventScroll: true })
  }, [ready])

  const back = (
    <LinkButton variant="quiet" href="#/" icon={<ChevronLeftIcon />}>
      Terug naar alle scenario's
    </LinkButton>
  )

  if (!catalog) {
    return (
      <div className={styles.page}>
        {catalogError && connected ? (
          <Callout tone="warn" title="Het overzicht kon niet geladen worden" actions={<Button onClick={actions.retryCatalog}>Opnieuw proberen</Button>}>
            {catalogError}
          </Callout>
        ) : (
          <SkeletonLines lines={5} label="Overzicht wordt geladen" />
        )}
      </div>
    )
  }
  if (!roadmap) {
    return (
      <div className={styles.page}>
        <h1 className={styles.h1}>Er is nog geen slotoverzicht</h1>
        <p className={styles.lead}>Niemand schreef er al een voor deze workshop.</p>
        <div>{back}</div>
      </div>
    )
  }

  return (
    <div className={styles.page}>
      <header className={styles.head}>
        <p className="kicker">Het einde van de workshop</p>
        <h1 className={styles.h1} id="part-heading" tabIndex={-1}>
          <RichText text={roadmap.title} />
        </h1>
        <p className={styles.lead}>
          <RichText text={roadmap.intro} />
        </p>
      </header>

      {view.total === 0 ? (
        <p className={styles.empty} role="status">
          Er is nog niets gemarkeerd als al in gebruik of als volgende stap.
        </p>
      ) : (
        view.sections.map((section) => <Section key={section.status} section={section} />)
      )}

      {room.counted > 0 && (
        <p className={styles.room} data-room-score="">
          <b>De zaal koos {room.agreed} van de {room.counted} keer wat het platform doet.</b> <a href="#/quiz">Bekijk de vragen</a>
        </p>
      )}

      {roadmap.closing && (
        <p className={styles.closing}>
          <RichText text={roadmap.closing} />
        </p>
      )}
      <div className={styles.actions}>{back}</div>
    </div>
  )
}
