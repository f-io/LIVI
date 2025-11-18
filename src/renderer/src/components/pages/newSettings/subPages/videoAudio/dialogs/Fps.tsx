import { FpsProps } from '../types'
import NumberSpinner from '../../../components/numberSpinner/numberSpinner'

export const Fps = ({ data, onChange }: FpsProps) => {
  return (
    <NumberSpinner
      id="fps"
      size="small"
      value={data.fps}
      onValueChange={(v) => onChange('fps', v)}
    />
  )
}
