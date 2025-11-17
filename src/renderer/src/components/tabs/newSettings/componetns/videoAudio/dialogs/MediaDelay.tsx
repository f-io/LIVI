import { TextField } from '@mui/material'
import { MediaDelayProps } from '../types'

export const MediaDelay = ({ data, onChange }: MediaDelayProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="Media delay"
      variant="outlined"
      type="number"
      value={data.mediaDelay}
      onChange={(e) => {
        onChange('mediaDelay', +e.target.value)
      }}
    />
  )
}
