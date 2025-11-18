import { TextField } from '@mui/material'
import { OemNameProps } from '../types'

export const OemName = ({ data, onChange }: OemNameProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="UI Label Name"
      variant="outlined"
      value={data.oemName}
      onChange={(e) => {
        onChange('oemName', e.target.value)
      }}
    />
  )
}
