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

/** An input's value in words ("Saturday 14 March 2026", "3 days", "On"), for "Recorded with ...". */
export function valueWords(input: StepInput, value: InputValue | undefined): string {
  const v = value ?? input.default
  switch (input.type) {
    case 'day':
      return formatDay(String(v), 'long')
    case 'number':
      return `${formatNumber(Number(v))}${input.unit ? ` ${input.unit}` : ''}`
    case 'toggle':
      return v === true ? 'On' : 'Off'
    default:
      return input.options?.find((o) => o.value === v)?.label ?? String(v)
  }
}
