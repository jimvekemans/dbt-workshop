import type { Visual } from '../api/types'
import { FlowVisual } from './FlowVisual'
import { TimelineVisual } from './TimelineVisual'
import { CompareVisual } from './CompareVisual'

export function VisualView({ visual, compact }: { visual: Visual; compact?: boolean }) {
  switch (visual.type) {
    case 'flow':
      return <FlowVisual visual={visual} compact={compact} />
    case 'timeline':
      return <TimelineVisual visual={visual} compact={compact} />
    case 'compare':
      return <CompareVisual visual={visual} compact={compact} />
  }
}
