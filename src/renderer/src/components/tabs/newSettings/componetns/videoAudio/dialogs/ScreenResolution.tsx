import { TextField } from '@mui/material'
import { ScreenResolutionProps } from '../types'

export const ScreenResolution = ({ data, onChange }: ScreenResolutionProps) => {
  return (
    <>
      <TextField
        fullWidth
        id="outlined-basic"
        label="Width"
        variant="outlined"
        type="number"
        value={data.width}
        onChange={(e) => {
          onChange('width', +e.target.value)
        }}
      />
      <span>X</span>
      <TextField
        fullWidth
        id="outlined-basic"
        label="Height"
        variant="outlined"
        type="number"
        value={data.height}
        onChange={(e) => {
          onChange('height', +e.target.value)
        }}
      />
    </>
  )
}
