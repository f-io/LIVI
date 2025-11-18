import NumberSpinner from '../../../components/numberSpinner/numberSpinner'
import { MediaDelayProps } from '../types'

export const MediaDelay = ({ data, onChange }: MediaDelayProps) => {
  return (
    <NumberSpinner
      id="media-delay"
      size="small"
      value={data.mediaDelay}
      onValueChange={(v) => onChange('mediaDelay', v)}
    />
  )
}
