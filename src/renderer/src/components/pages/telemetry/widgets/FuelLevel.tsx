import { MetricRow } from '../components/MetricRow'

export type FuelLevelProps = {
  fuelPct?: number
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export function FuelLevel({ fuelPct = 0, className }: FuelLevelProps) {
  const v = Number.isFinite(fuelPct) ? clamp(Math.round(fuelPct), 0, 100) : 0

  return (
    <MetricRow
      className={className}
      label="FUEL"
      value={v}
      unit="%"
      min={0}
      max={100}
      barValue={v}
      warnBelow={11}
      valueWidthCh={3}
      labelAlign="right"
    />
  )
}
