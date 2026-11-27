import { beforeEach, describe, expect, it } from 'vitest'
import { CATALOG } from '../dev/mockContent'
import { withFlowAndRoadmap, withQuiz } from '../dev/mockFlow'
import { QUIZ_KEY, isPlayed, isWaiting, nextItem, proofHref, questionHref, quizActions, quizItems, readQuiz, routeMinutes, score, tally, timing, votesText } from './quiz'

const catalog = withQuiz(withFlowAndRoadmap(CATALOG, () => true))
const items = quizItems(catalog)
const byKey = (key: string) => items.find((i) => i.key === key)!

beforeEach(() => localStorage.clear())

describe('the questions of the catalog', () => {
  it('lists the question of a scenario and of each step that has one, in the order of the home page', () => {
    expect(items.map((i) => i.key)).toEqual(['late-delivery', 'skip-one-table', 'flow-test/check', 'flow-test/try', 'flow-test/release'])
    expect(quizItems(null)).toEqual([])
    expect(quizItems(CATALOG)).toEqual([])
  })
  it('gives a scenario its own minutes and a stage an even share of its flow', () => {
    const one = byKey('skip-one-table')
    expect([one.talk, one.demo, one.minutes]).toEqual([2, 4, 6])
    const stage = byKey('flow-test/try')
    expect([stage.stepIndex, stage.stepCount, stage.talk, stage.demo]).toEqual([3, 6, 3, 4])
    expect(stage.status).toBe('next')
    expect(stage.feature).toBe('Private test copy')
  })
  it('carries what really happened, for a scenario and for a stage, and null without it', () => {
    expect(byKey('skip-one-table').reality).toMatch(/werkte ze niet meer zoals bedoeld/)
    expect(byKey('flow-test/try').reality).toMatch(/vijf keer goedgekeurd/)
    expect(byKey('late-delivery').reality).toBeNull()
    expect(byKey('flow-test/check').reality).toBeNull()
  })
  it('knows where a question is asked and where its proof is', () => {
    expect(questionHref(byKey('skip-one-table'))).toBe('#/quiz/skip-one-table')
    expect(proofHref(byKey('skip-one-table'))).toBe('#/s/skip-one-table/problem')
    expect(questionHref(byKey('flow-test/try'))).toBe('#/quiz/flow-test/try')
    expect(proofHref(byKey('flow-test/try'))).toBe('#/s/flow-test/demo?step=try')
  })
})

describe('counting hands', () => {
  it('counts up and down, never below zero, and keeps it in this browser', () => {
    quizActions.vote('skip-one-table', 2, 1, 3)
    quizActions.vote('skip-one-table', 2, 1, 3)
    quizActions.vote('skip-one-table', 0, -1, 3)
    quizActions.vote('skip-one-table', 5, 1, 3)
    expect(readQuiz().answers['skip-one-table']!.votes).toEqual([0, 0, 2])
    expect(JSON.parse(localStorage.getItem(QUIZ_KEY)!).answers['skip-one-table'].votes).toEqual([0, 0, 2])
  })
  it('says what the room chose and whether that is the choice of the platform', () => {
    const q = byKey('skip-one-table').quiz
    expect(tally(q, undefined)).toEqual({ votes: [0, 0, 0], total: 0, top: [], agrees: false })
    const clear = tally(q, { votes: [1, 2, 6], revealed: true, skipped: false })
    expect(clear.top).toEqual([2])
    expect(clear.agrees).toBe(true)
    expect(votesText(clear)).toBe('A 1 · B 2 · C 6')
    const tie = tally(q, { votes: [4, 0, 4], revealed: true, skipped: false })
    expect(tie.top).toEqual([0, 2])
    expect(tie.agrees).toBe(false)
    expect(tally(q, { votes: [5, 1, 0], revealed: true, skipped: false }).agrees).toBe(false)
  })
  it('scores only questions that were revealed with hands counted', () => {
    quizActions.vote('skip-one-table', 2, 1, 3)
    quizActions.reveal('skip-one-table')
    quizActions.vote('late-delivery', 1, 1, 2)
    quizActions.reveal('late-delivery')
    quizActions.reveal('flow-test/check')
    quizActions.vote('flow-test/try', 1, 1, 3)
    expect(score(items, readQuiz())).toEqual({ counted: 2, agreed: 1, played: 3 })
  })
  it('survives nonsense in storage', () => {
    localStorage.setItem(QUIZ_KEY, '{"plan":"x","answers":{"a":{"votes":[-3,"y",2.7]}},"budget":"lots"}')
    const s = readQuiz()
    expect(s.plan).toEqual([])
    expect(s.answers.a).toEqual({ votes: [0, 0, 2], revealed: false, skipped: false })
    expect(s.budget).toBe(120)
    localStorage.setItem(QUIZ_KEY, 'not json')
    expect(readQuiz().opening).toBe(true)
  })
})

describe('opening a scenario and moving on', () => {
  it('waits for a question until its impacts were shown or it was skipped', () => {
    expect(isWaiting(readQuiz(), 'skip-one-table')).toBe(true)
    quizActions.vote('skip-one-table', 0, 1, 3)
    expect(isWaiting(readQuiz(), 'skip-one-table')).toBe(true)
    quizActions.reveal('skip-one-table')
    expect(isPlayed(readQuiz(), 'skip-one-table')).toBe(true)
    expect(isWaiting(readQuiz(), 'skip-one-table')).toBe(false)
    quizActions.skip('late-delivery')
    expect(isWaiting(readQuiz(), 'late-delivery')).toBe(false)
    quizActions.again('late-delivery')
    expect(isWaiting(readQuiz(), 'late-delivery')).toBe(true)
  })
  it('names the next question: the next one of the route that was not played, else the first open one', () => {
    expect(nextItem(items, readQuiz())?.key).toBe('late-delivery')
    expect(nextItem(items, readQuiz(), 'skip-one-table')?.key).toBe('flow-test/check')
    quizActions.setPlanned(['flow-test/release', 'late-delivery'], true)
    expect(nextItem(items, readQuiz(), 'skip-one-table')?.key).toBe('flow-test/release')
    expect(nextItem(items, readQuiz(), 'flow-test/release')?.key).toBe('late-delivery')
    quizActions.reveal('late-delivery')
    quizActions.skip('flow-test/release')
    expect(nextItem(items, readQuiz())).toBeNull()
  })
})

describe('the time a route takes', () => {
  it('counts the earlier steps of a demo too, because steps run in order', () => {
    const s = readQuiz()
    expect(routeMinutes([byKey('skip-one-table')], s)).toBe(6)
    // step 5 of 6, 4 minutes a step: five steps of demo and one question
    expect(routeMinutes([byKey('flow-test/release')], s)).toBe(2 + 5 * 4)
    // two questions of the same flow share the steps before them
    expect(routeMinutes([byKey('flow-test/check'), byKey('flow-test/release')], s)).toBe(2 + 2 + 5 * 4)
  })
  it('counts only what is still to play once questions are behind us', () => {
    quizActions.reveal('flow-test/check')
    const s = readQuiz()
    // step 2 is behind us: steps 3, 4 and 5 remain
    expect(routeMinutes([byKey('flow-test/check'), byKey('flow-test/release')], s, true)).toBe(2 + 3 * 4)
  })
  it('compares the route with the session, and with what is left once the clock runs', () => {
    quizActions.setPlanned(['skip-one-table', 'late-delivery'], true)
    quizActions.setBudget(30)
    let t = timing(items, readQuiz())
    expect([t.picked, t.questions, t.plannedMinutes, t.elapsed, t.slack]).toEqual([true, 2, 10, null, 20])
    quizActions.startClock(0)
    quizActions.reveal('skip-one-table')
    t = timing(items, readQuiz(), 21 * 60000)
    expect([t.elapsed, t.toGoMinutes, t.slack]).toEqual([21, 4, 5])
    quizActions.clearAnswers()
    expect(readQuiz().startedAt).toBeNull()
    expect(readQuiz().plan).toEqual(['skip-one-table', 'late-delivery'])
  })
  it('counts every question while no route is picked', () => {
    const t = timing(items, readQuiz())
    expect([t.picked, t.questions]).toEqual([false, 5])
  })
})
