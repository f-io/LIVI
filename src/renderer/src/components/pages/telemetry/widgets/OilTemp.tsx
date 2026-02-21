import { MetricRow } from '../components/MetricRow'

export type OilTempProps = {
  oilC?: number
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export function OilTemp({ oilC = 0, className }: OilTempProps) {
  const v = Number.isFinite(oilC) ? clamp(Math.round(oilC), -99, 999) : 0

  return (
    <MetricRow
      className={className}
      label="OIL"
      value={v}
      unit="°C"
      min={0}
      max={160}
      barValue={v}
      warnFrom={130}
      valueWidthCh={3}
      labelAlign="right"
    />
  )
}
