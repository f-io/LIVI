import NumberSpinner from '../../../components/numberSpinner/numberSpinner'
import { AudioVolumeProps } from '../types'

export const AudioVolume = ({ data, onChange }: AudioVolumeProps) => {
  return (
    <NumberSpinner
      id="audio-volume"
      size="small"
      value={data.audioVolume}
      onValueChange={(v) => onChange('audioVolume', v)}
    />
  )
}
