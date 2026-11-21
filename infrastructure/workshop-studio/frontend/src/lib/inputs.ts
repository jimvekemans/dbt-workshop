import type { InputValue, InputValues, StepInput } from '../api/types'
import { formatDay, formatNumber } from './format'

export function defaultsFor(inputs: StepInput[]): InputValues {
  const out: InputValues = {}
  for (const i of inputs) out[i.id] = i.default
  return out
}

/** The help line of an input: one sentence, or one per state for a toggle. */
export function helpText(help: StepInput['help'], on: boolean): string | undefined {
  if (!help) return undefined
  return typeof help === 'string' ? help : on ? help.on : help.off
}

/** An input's value in words ("zaterdag 14 maart 2026", "3 dagen", "Aan"), for "Vast in de opname: ...". */
export function valueWords(input: StepInput, value: InputValue | undefined): string {
  const v = value ?? input.default
  switch (input.type) {
    case 'day':
      return formatDay(String(v), 'long')
    case 'number':
      return `${formatNumber(Number(v))}${input.unit ? ` ${input.unit}` : ''}`
    case 'toggle':
      return v === true ? 'Aan' : 'Uit'
    default:
      return input.options?.find((o) => o.value === v)?.label ?? String(v)
  }
}
