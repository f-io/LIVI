import { TextField } from '@mui/material'
import { CarNameProps } from '../types'

export const CarName = ({ data, onChange }: CarNameProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="Car Name"
      variant="outlined"
      value={data.carName}
      onChange={(e) => {
        onChange('carName', e.target.value)
      }}
    />
  )
}
