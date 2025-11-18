import NumberSpinner from '../../../components/numberSpinner/numberSpinner'
import { ScreenResolutionProps } from '../types'

export const ScreenResolution = ({ data, onChange }: ScreenResolutionProps) => {
  return (
    <>
      <NumberSpinner
        id="width"
        label="Width"
        size="small"
        value={data.width}
        onValueChange={(v) => onChange('width', v)}
      />
      <span> </span>
      <NumberSpinner
        id="height"
        label="Height"
        size="small"
        value={data.height}
        onValueChange={(v) => onChange('height', v)}
      />
    </>
  )
}
