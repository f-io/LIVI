export type RpmRingProps = {
  rpm: number
  maxRpm: number
  redlineRpm: number

  ticks?: number
  arcDeg?: number
  startDeg?: number

  colorOff: string
  colorOn: string
  colorRedline: string

  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

export function RpmRing({
  rpm,
  maxRpm,
  redlineRpm,
  ticks = 44,
  arcDeg = 240,
  startDeg = 90,
  colorOff,
  colorOn,
  colorRedline,
  className
}: RpmRingProps) {
  const safeTicks = Math.max(2, Math.floor(ticks))
  const safeMax = Math.max(1, maxRpm)
  const safeRed = clamp(redlineRpm, 0, safeMax)

  const ratio = clamp(rpm / safeMax, 0, 1)
  const activeTicks = clamp(Math.round(ratio * safeTicks), 0, safeTicks)

  const redStartTick = clamp(Math.floor((safeRed / safeMax) * safeTicks), 0, safeTicks)
  const step = arcDeg / (safeTicks - 1)

  const r = 50
  const tickW = 2.4
  const tickH = 11.2
  const pad = Math.max(tickH, tickW) + 6

  const vbMin = -(r + pad)
  const vbSize = (r + pad) * 2

  return (
    <svg
      className={className}
      viewBox={`${vbMin} ${vbMin} ${vbSize} ${vbSize}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="RPM ring"
      style={{ width: '100%', height: '100%', display: 'block', overflow: 'visible' }}
    >
      <g>
        {Array.from({ length: safeTicks }).map((_, i) => {
          const deg = startDeg + i * step
          const ang = (deg * Math.PI) / 180

          const x = Math.cos(ang) * r
          const y = Math.sin(ang) * r
          const rot = deg + 90

          const isOn = i < activeTicks
          const isRedZone = i >= redStartTick
          const fill = !isOn ? colorOff : isRedZone ? colorRedline : colorOn

          return (
            <rect
              key={i}
              x={x - tickW / 2}
              y={y - tickH / 2}
              width={tickW}
              height={tickH}
              rx={1}
              fill={fill}
              transform={`rotate(${rot} ${x} ${y})`}
            />
          )
        })}
      </g>
    </svg>
  )
}
