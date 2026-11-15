import type { InputValue, StepInput } from '../api/types'
import { DayPicker } from './DayPicker'
import { Slider } from './Slider'
import { ChoiceButtons } from './ChoiceButtons'
import { Toggle } from './Toggle'
import { formatNumber } from '../lib/format'
import { helpText } from '../lib/inputs'
import styles from './InputField.module.css'

interface Props {
  input: StepInput
  value: InputValue | undefined
  onChange: (v: InputValue) => void
  disabled?: boolean
  /** Replay: the recording was made with the default; it cannot be changed. */
  replay?: boolean
}

/** Renders the right control for one input of a step. */
export function InputField({ input, value, onChange, disabled, replay }: Props) {
  const off = disabled || replay
  const control = () => {
    switch (input.type) {
      case 'day': {
        const dflt = String(input.default)
        return <DayPicker label={input.label} help={helpText(input.help, true)} min={String(input.min ?? dflt)} max={String(input.max ?? dflt)} value={typeof value === 'string' ? value : dflt} mark={input.mark ?? undefined} onChange={onChange} disabled={off} />
      }
      case 'number': {
        const dflt = Number(input.default)
        const v = typeof value === 'number' ? value : dflt
        const min = Number(input.min ?? dflt)
        const max = Number(input.max ?? dflt)
        const unit = input.unit ? ` ${input.unit}` : ''
        return (
          <Slider
            label={input.label}
            help={helpText(input.help, true)}
            min={min}
            max={max}
            step={input.step ?? 1}
            value={v}
            display={`${formatNumber(v)}${unit}`}
            minLabel={`${formatNumber(min)}${unit}`}
            maxLabel={`${formatNumber(max)}${unit}`}
            onChange={onChange}
            disabled={off}
          />
        )
      }
      case 'choice':
        return <ChoiceButtons label={input.label} help={helpText(input.help, true)} options={input.options ?? []} value={typeof value === 'string' ? value : String(input.default)} onChange={onChange} disabled={off} />
      case 'toggle': {
        const checked = typeof value === 'boolean' ? value : input.default === true
        return <Toggle label={input.label} help={helpText(input.help, checked)} checked={checked} onChange={onChange} disabled={off} />
      }
    }
  }
  return (
    <div className={styles.field}>
      {control()}
    </div>
  )
}
