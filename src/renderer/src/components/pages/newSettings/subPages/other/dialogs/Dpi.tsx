import { DpiProps } from '../types'
import NumberSpinner from '../../../components/numberSpinner/numberSpinner'

export const Dpi = ({ data, onChange }: DpiProps) => {
  return (
    <NumberSpinner
      id="dpi"
      size="small"
      value={data.dpi}
      onValueChange={(v) => onChange('dpi', v)}
    />
  )
}
