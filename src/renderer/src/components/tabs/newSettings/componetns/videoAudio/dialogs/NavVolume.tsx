import { TextField } from '@mui/material'
import { NavVolumeProps } from '../types'

export const NavVolume = ({ data, onChange }: NavVolumeProps) => {
  return (
    <TextField
      fullWidth
      id="outlined-basic"
      label="Nav Volume"
      variant="outlined"
      type="number"
      value={Math.round((data.navVolume || 0) * 100)}
      onChange={(e) => {
        onChange('navVolume', +e.target.value)
      }}
    />
  )
}
