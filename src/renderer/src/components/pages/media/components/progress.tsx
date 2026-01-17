import { msToClock } from '../../../../utils'

type ProgressProps = {
  elapsedMs: number
  progressH: number
  totalMs: number
  pct: number
}

export const ProgressBar = ({ elapsedMs, progressH, totalMs, pct }: ProgressProps) => {
  const left = msToClock(elapsedMs)
  const right = `-${msToClock(Math.max(0, totalMs - elapsedMs))}`

  return (
    <div
      style={{
        width: '100%',
        maxWidth: '100%',
        display: 'grid',
        gridTemplateColumns: 'minmax(3.5em, max-content) minmax(0, 1fr) minmax(3.5em, max-content)',
        alignItems: 'center',
        columnGap: 12,
        boxSizing: 'border-box'
      }}
    >
      <div
        style={{
          fontSize: 14,
          opacity: 0.85,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'clip'
        }}
      >
        {left}
      </div>

      <div
        style={{
          minWidth: 0,
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
            background: 'var(--ui-highlight)'
          }}
        />
      </div>

      <div
        style={{
          fontSize: 14,
          opacity: 0.85,
          textAlign: 'right',
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'clip'
        }}
      >
        {right}
      </div>
    </div>
  )
}
