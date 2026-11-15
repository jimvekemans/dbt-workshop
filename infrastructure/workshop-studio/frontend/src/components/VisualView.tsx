import type { Visual } from '../api/types'
import { FlowVisual } from './FlowVisual'
import { TimelineVisual } from './TimelineVisual'
import { CompareVisual } from './CompareVisual'

export function VisualView({ visual }: { visual: Visual }) {
  switch (visual.type) {
    case 'flow':
      return <FlowVisual visual={visual} />
    case 'timeline':
      return <TimelineVisual visual={visual} />
    case 'compare':
      return <CompareVisual visual={visual} />
  }
}
