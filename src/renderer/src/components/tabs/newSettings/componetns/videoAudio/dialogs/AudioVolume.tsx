import { TextField } from '@mui/material'
import { AudioVolumeProps } from '../types'

export const AudioVolume = ({ data, onChange }: AudioVolumeProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="Audio Volume"
      variant="outlined"
      type="number"
      value={Math.round((data.audioVolume || 0) * 100)}
      onChange={(e) => {
        onChange('audioVolume', +e.target.value)
      }}
    />
  )
}
