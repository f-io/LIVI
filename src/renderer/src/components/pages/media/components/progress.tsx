import { msToClock } from '../../../../utils'

type ProgressProps = {
  elapsedMs: number
  progressH: number
  totalMs: number
  pct: number
}

export const ProgressBar = ({ elapsedMs, progressH, totalMs, pct }: ProgressProps) => {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '80px 1fr 80px',
        alignItems: 'center',
        columnGap: 12
      }}
    >
      <div style={{ fontSize: 14, opacity: 0.85 }}>{msToClock(elapsedMs)}</div>
      <div
        style={{
          height: progressH,
          borderRadius: progressH / 1.6,
          background: 'rgba(255,255,255,0.28)',
          overflow: 'hidden'
        }}
      >
        <div
          style={{
            width: `${pct}%`,
            height: '100%',
            transition: 'width 120ms linear',
            background: 'rgba(255,255,255,0.95)'
          }}
        />
      </div>
      <div style={{ fontSize: 14, opacity: 0.85, textAlign: 'right' }}>
        -{msToClock(Math.max(0, totalMs - elapsedMs))}
      </div>
    </div>
  )
}
