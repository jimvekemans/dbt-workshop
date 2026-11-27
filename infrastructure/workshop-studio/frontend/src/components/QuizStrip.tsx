import { useMemo } from 'react'
import { useStudio } from '../lib/studio'
import { isPlayed, itemKey, questionHref, quizItems, tally, useQuiz, votesText } from '../lib/quiz'
import { plainText } from '../lib/glossary'
import { LinkButton } from './LinkButton'
import styles from './QuizStrip.module.css'

/**
 * The question that belongs to a scenario or to one of its steps, as one line: a button to ask it, or what the room
 * chose once it has been asked. Nothing when there is no question.
 */
export function QuizStrip({ scenarioId, stepId }: { scenarioId: string; stepId?: string | null }) {
  const { catalog } = useStudio()
  const state = useQuiz()
  const key = itemKey(scenarioId, stepId)
  const item = useMemo(() => quizItems(catalog).find((i) => i.key === key) ?? null, [catalog, key])
  if (!item) return null
  const played = isPlayed(state, key)
  const t = tally(item.quiz, state.answers[key])
  return (
    <p className={styles.strip} data-quiz-strip={played ? 'played' : 'open'}>
      <span className={styles.label}>Vraag aan de zaal</span>
      <span className={styles.text}>{plainText(item.quiz.question)}</span>
      {played ? (
        <>
          {t.total > 0 && <span className={styles.votes}>De zaal koos: {votesText(t)}</span>}
          <a className={styles.link} href={questionHref(item)}>
            Bekijk de vraag
          </a>
        </>
      ) : (
        <LinkButton href={questionHref(item)}>Stel de vraag</LinkButton>
      )}
    </p>
  )
}
