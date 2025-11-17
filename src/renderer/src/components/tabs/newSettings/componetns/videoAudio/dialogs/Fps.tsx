import { TextField } from '@mui/material'
import { FpsProps } from '../types'

export const Fps = ({ data, onChange }: FpsProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="FPS"
      variant="outlined"
      type="number"
      value={data.fps}
      onChange={(e) => {
        onChange('fps', +e.target.value)
      }}
    />
  )
}
