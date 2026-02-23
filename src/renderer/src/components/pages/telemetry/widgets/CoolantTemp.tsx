import { MetricRow } from '../components/MetricRow'

export type CoolantTempProps = {
  coolantC?: number
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export function CoolantTemp({ coolantC = 0, className }: CoolantTempProps) {
  const v = Number.isFinite(coolantC) ? clamp(Math.round(coolantC), -99, 999) : 0

  return (
    <MetricRow
      className={className}
      label="COOLANT"
      value={v}
      unit="°C"
      min={0}
      max={140}
      barValue={v}
      warnFrom={110}
      valueWidthCh={3}
      labelAlign="right"
    />
  )
}
