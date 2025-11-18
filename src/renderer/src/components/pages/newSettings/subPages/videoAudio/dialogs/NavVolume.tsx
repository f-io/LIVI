import NumberSpinner from '../../../components/numberSpinner/numberSpinner'
import { NavVolumeProps } from '../types'

export const NavVolume = ({ data, onChange }: NavVolumeProps) => {
  return (
    <NumberSpinner
      id="nav-volume"
      size="small"
      value={data.navVolume}
      onValueChange={(v) => onChange('navVolume', v)}
    />
  )
}
