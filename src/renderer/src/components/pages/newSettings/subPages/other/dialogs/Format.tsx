import NumberSpinner from '../../../components/numberSpinner/numberSpinner'
import { FormatProps } from '../types'

export const Format = ({ data, onChange }: FormatProps) => {
  return (
    <NumberSpinner
      id="format"
      size="small"
      value={data.format}
      onValueChange={(v) => onChange('format', v)}
    />
  )
}
