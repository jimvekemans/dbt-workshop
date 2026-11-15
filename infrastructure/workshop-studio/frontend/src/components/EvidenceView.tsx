import { plainText } from '../lib/glossary'
import { useRef } from 'react'
import type {
  BarsData, EvidenceType, FreshnessData, LiveEvidence, MailData, NumberData, PipelineData, ResultData, RunHistoryData, StepStatus,
  TableData, TasksData, VersionsData,
} from '../api/types'
import { barsModel, dataMatches, evidenceError } from '../api/adapt'
import type { VersionTags } from '../api/adapt'
import { EvidenceFrame } from './EvidenceFrame'
import type { EvidencePhase } from './EvidenceFrame'
import { TableView } from './TableView'
import { BarsView } from './BarsView'
import { NumberView } from './NumberView'
import { TasksView } from './TasksView'
import { PipelineView } from './PipelineView'
import { MailView } from './MailView'
import { VersionsView } from './VersionsView'
import { ResultView } from './ResultView'
import { FreshnessView } from './FreshnessView'
import { RunHistoryView } from './RunHistoryView'

const SIZE: Record<EvidenceType, 'sm' | 'md' | 'lg'> = {
  table: 'md', bars: 'lg', number: 'sm', tasks: 'md', pipeline: 'sm', mail: 'lg', versions: 'md', result: 'sm', freshness: 'md', run_history: 'md',
}
/** Used when a scenario file leaves the title out. */
const TITLE: Record<EvidenceType, string> = {
  table: 'The numbers', bars: 'The numbers per day', number: 'The number', tasks: 'The work, step by step', pipeline: 'The automatic checks',
  mail: 'The mail', versions: 'Version in each environment', result: 'What happened', freshness: 'How fresh is each feed', run_history: 'The latest runs',
}

export function evidencePhase(item: LiveEvidence, stepStatus: StepStatus): EvidencePhase {
  if (evidenceError(item.data) !== null) return 'error'
  if (item.data && !dataMatches(item.type, item.data)) return 'error'
  if (item.data) return 'ready'
  return stepStatus === 'running' ? 'loading' : 'pending'
}

/** Renders one piece of evidence in its frame. `stepStatus` decides whether it is waiting, loading or updating. */
export function EvidenceView({ item, stepStatus, previous, trail, waitingText }: { item: LiveEvidence; stepStatus: StepStatus; previous?: VersionTags; trail?: number[]; waitingText?: string }) {
  const phase = evidencePhase(item, stepStatus)
  // every slot of a bar chart is drawn from the first moment: remember the longest label list seen so far
  const slots = useRef<string[]>([])
  // ... and so is every series of the legend (a series that is empty now still keeps its place, with no bars)
  const seen = useRef<{ name: string; state: BarsData['series'][number]['state'] }[]>([])
  const rawTitle = item.title || TITLE[item.type]
  const title = plainText(rawTitle)
  const d = item.data
  if (item.type === 'bars' && phase === 'ready' && d && 'series' in d) {
    const lab = barsModel(d as BarsData).labels
    if (lab.length >= slots.current.length) slots.current = lab
    for (const s of (d as BarsData).series) {
      const name = s.name || 'Total'
      if (!seen.current.some((x) => x.name === name)) seen.current = [...seen.current, { name, state: s.state }]
    }
    // the same order in every chart: what is as it should be first, the problem last
    const rank = (st: string) => (st === 'good' ? 0 : st === 'bad' ? 2 : 1)
    seen.current = [...seen.current].sort((x, y) => rank(x.state) - rank(y.state))
  }
  let body: React.ReactNode = null
  if (phase === 'ready' && d) {
    switch (item.type) {
      case 'table': body = <TableView item={item} data={d as TableData} title={title} />; break
      case 'bars': {
        const bars = d as BarsData
        const full: BarsData = { ...bars, series: seen.current.map((x) => bars.series.find((s) => (s.name || 'Total') === x.name) ?? { name: x.name, state: x.state, values: [] }) }
        const unit = [bars.unit, item.unit].find((u): u is string => typeof u === 'string' && u !== '')
        body = <BarsView data={full} title={title} slots={slots.current} unit={unit} />
        break
      }
      case 'number': body = <NumberView data={d as NumberData} trail={trail} />; break
      case 'tasks': body = <TasksView data={d as TasksData} />; break
      case 'pipeline': body = <PipelineView data={d as PipelineData} />; break
      case 'mail': body = <MailView data={d as MailData} recorded={!!item.recorded_at} />; break
      case 'versions': body = <VersionsView data={d as VersionsData} previous={previous} />; break
      case 'result': body = <ResultView data={d as ResultData} />; break
      case 'freshness': body = <FreshnessView data={d as FreshnessData} />; break
      case 'run_history': body = <RunHistoryView data={d as RunHistoryData} limit={typeof item.limit === 'number' ? item.limit : undefined} />; break
    }
  }
  return (
    <EvidenceFrame title={rawTitle} caption={item.caption ?? undefined} phase={phase} updating={stepStatus === 'running' && phase === 'ready'} error={phase === 'error' ? (evidenceError(d) ?? 'What was recorded does not fit this scenario any more.') : null} size={SIZE[item.type]} recordedAt={item.recorded_at} waitingText={waitingText} guide={item.type === 'mail'}>
      {body}
    </EvidenceFrame>
  )
}
