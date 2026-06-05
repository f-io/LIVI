import { useEffect, useReducer, useRef } from 'react'

export type GaugeArcProps = {
  /** Current value (drives the running pointer). */
  value: number
  /** Top-of-scale value (pointer pins here). */
  scaleMax: number
  /** Value at/above which the pointer + scale turn red (0 = no redline). */
  redline?: number
  /** Minor tick count along the arc. */
  ticks?: number
  /** Circle radius (round outer edge). */
  radius?: number
  /** Angular opening toward the centre, in degrees (180 = exact half-circle). */
  gapDeg?: number
  /** Horizontal ticks per cap (top + bottom), mirrored for symmetry. */
  armTicks?: number
  /** Number of labelled major graduations along the arc (bottom → top). */
  majorCount?: number
  /** Label text per major, bottom → top. Length should match majorCount. */
  labels?: string[]
  /** Flip horizontally without mirroring the text (for the right ring). */
  mirror?: boolean
  /** Peak opacity of the translucent trail covering the passed-over scale (0 → pointer). */
  trailMax?: number
  tickW?: number
  tickH?: number
  majorH?: number
  labelSize?: number
  colorScale: string
  colorMajor: string
  colorPointer: string
  colorRedline: string
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))
const rad = (d: number) => (d * Math.PI) / 180

// Pointer start position for the boot sweep: just below the 0 mark, on the same circle.
const START_T = -0.08

type Tick = { x: number; y: number; ang: number; t: number | null; capK: number }

/**
 * Tick geometry for a round arc (radius R, opening `gapDeg` toward the centre) capped at top and
 * bottom by `armTicks` horizontal ticks. `t` is the 0..1 position along the arc (bottom → top);
 * caps carry `t = null` and `capK` = 1 (next to the arc) … armTicks (outermost).
 */
function gaugeTicks(R: number, gapDeg: number, arcTicks: number, caps: number): Tick[] {
  const g = gapDeg / 2
  const xEnd = R * Math.cos(rad(g))
  const yLo = R * Math.sin(rad(g))
  const arcSpan = 360 - gapDeg
  const M = Math.max(2, arcTicks)
  const armPitch = ((arcSpan / 360) * 2 * Math.PI * R) / (M - 1)

  const out: Tick[] = []
  for (let k = caps; k >= 1; k--)
    out.push({ x: xEnd + k * armPitch, y: yLo, ang: 90, t: null, capK: k })
  for (let i = 0; i < M; i++) {
    const t = i / (M - 1)
    const a = g + t * arcSpan
    out.push({ x: R * Math.cos(rad(a)), y: R * Math.sin(rad(a)), ang: a, t, capK: 0 })
  }
  for (let k = 1; k <= caps; k++)
    out.push({ x: xEnd + k * armPitch, y: -yLo, ang: 270, t: null, capK: k })
  return out
}

/** Arc point + outward-normal angle at fractional position t (0 = bottom, 1 = top); unclamped. */
function arcAt(R: number, gapDeg: number, t: number) {
  const a = gapDeg / 2 + t * (360 - gapDeg)
  return { x: R * Math.cos(rad(a)), y: R * Math.sin(rad(a)), ang: a }
}

/**
 * Round gauge: a minimalist tick scale with labelled majors and a pointer that runs
 * along the arc. The passed-over scale (0 → pointer) is overlaid by a translucent gradient so the
 * eye lands on the pointer instantly. The caps below 0 fade out. On mount the pointer sweeps up
 * from just below 0 and fades in.
 */
export function GaugeArc({
  value,
  scaleMax,
  redline = 0,
  ticks = 41,
  radius = 110,
  gapDeg = 180,
  armTicks = 3,
  majorCount = 6,
  labels = [],
  mirror = false,
  trailMax = 0.5,
  tickW = 3,
  tickH = 14,
  majorH = 26,
  labelSize = 15,
  colorScale,
  colorMajor,
  colorPointer,
  colorRedline,
  className
}: GaugeArcProps) {
  const arcTicks = Math.max(2, Math.floor(ticks))
  const caps = Math.max(0, Math.floor(armTicks))
  const pts = gaugeTicks(radius, gapDeg, arcTicks, caps)

  const safeMax = Math.max(1, scaleMax)
  const targetT = clamp(value / safeMax, 0, 1)
  const redT = redline > 0 ? clamp(redline / safeMax, 0, 1) : 2

  // Eased pointer + fade-in. Starting below 0 gives the boot sweep. Value changes
  // ease smoothly toward the new target.
  const disp = useRef({ t: START_T, op: 0 })
  const target = useRef(targetT)
  target.current = targetT
  const raf = useRef(0)
  const [, force] = useReducer((x: number) => x + 1, 0)
  useEffect(() => {
    const tick = () => {
      const tt = target.current
      const d = disp.current
      const nt = d.t + (tt - d.t) * 0.16
      const no = d.op + (1 - d.op) * 0.09
      const moving = Math.abs(nt - tt) > 0.0004 || 1 - no > 0.01
      disp.current = moving ? { t: nt, op: no } : { t: tt, op: 1 }
      force()
      raf.current = moving ? requestAnimationFrame(tick) : 0
    }
    if (!raf.current) raf.current = requestAnimationFrame(tick)
    return () => {
      if (raf.current) cancelAnimationFrame(raf.current)
      raf.current = 0
    }
  }, [targetT])

  const dT = disp.current.t
  const dOp = disp.current.op
  const pointerIdx = dT * (arcTicks - 1)
  const redIdx = redT * (arcTicks - 1)

  const majors = new Set<number>()
  if (majorCount >= 2) {
    for (let m = 0; m < majorCount; m++) {
      majors.add(Math.round((m / (majorCount - 1)) * (arcTicks - 1)))
    }
  }

  const sx = mirror ? -1 : 1
  const place = (x: number, y: number, ang: number) => ({
    px: x * sx,
    py: y,
    pang: mirror ? 180 - ang : ang
  })
  const capFade = (k: number) => (caps <= 1 ? 1 : 0.18 + 0.72 * ((caps - k) / (caps - 1)))

  // viewBox centred on the tick content (so the readout sits on the gauge centre), padded for
  // the outer labels.
  const armPitch = (((360 - gapDeg) / 360) * 2 * Math.PI * radius) / (arcTicks - 1)
  const contentLeft = -radius
  const contentRight = radius * Math.cos(rad(gapDeg / 2)) + caps * armPitch
  const cx = (contentLeft + contentRight) / 2
  const labelReach = labelSize + 26
  const hw = (contentRight - contentLeft) / 2 + labelReach
  const hh = radius + labelReach

  let scaleArc = -1
  let trailArc = -1

  return (
    <svg
      className={className}
      viewBox={`${sx * cx - hw} ${-hh} ${hw * 2} ${hh * 2}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="gauge"
      style={{ width: '100%', height: '100%', display: 'block', overflow: 'visible' }}
    >
      {/* scale: caps (faded), minor + major arc ticks */}
      {pts.map((p, i) => {
        const isArc = p.t !== null
        if (isArc) scaleArc++
        const j = isArc ? scaleArc : -1
        const isMajor = isArc && majors.has(j)
        const inRed = isArc && j >= redIdx
        const base = inRed ? colorRedline : isMajor ? colorMajor : colorScale
        const op = isArc ? (inRed && !isMajor ? 0.55 : 1) : capFade(p.capK)
        const h = isMajor ? majorH : tickH
        const { px, py, pang } = place(p.x, p.y, p.ang)
        const cxk = isMajor ? px - Math.cos(rad(pang)) * ((h - tickH) / 2) : px
        const cyk = isMajor ? py - Math.sin(rad(pang)) * ((h - tickH) / 2) : py
        return (
          <rect
            key={`s${i}`}
            x={cxk - tickW / 2}
            y={cyk - h / 2}
            width={tickW}
            height={h}
            rx={1.2}
            fill={base}
            fillOpacity={op}
            transform={`rotate(${pang + 90} ${cxk} ${cyk})`}
          />
        )
      })}

      {/* translucent trail over the passed-over scale: faint at 0, strongest at the pointer */}
      {pts.map((p, i) => {
        if (p.t === null) return null
        trailArc++
        if (trailArc > pointerIdx) return null
        const frac = pointerIdx > 0 ? trailArc / pointerIdx : 0
        const op = dOp * trailMax * frac
        if (op < 0.02) return null
        const { px, py, pang } = place(p.x, p.y, p.ang)
        return (
          <rect
            key={`t${i}`}
            x={px - tickW / 2}
            y={py - tickH / 2}
            width={tickW}
            height={tickH}
            rx={1.2}
            fill={trailArc >= redIdx ? colorRedline : colorPointer}
            fillOpacity={op}
            transform={`rotate(${pang + 90} ${px} ${py})`}
          />
        )
      })}

      {/* pointer needle at the (eased) value, growing inward, fading in on boot */}
      {(() => {
        const nip = arcAt(radius, gapDeg, dT)
        const { px, py, pang } = place(nip.x, nip.y, nip.ang)
        const nH = majorH + 12
        const cxn = px - Math.cos(rad(pang)) * ((nH - tickH) / 2)
        const cyn = py - Math.sin(rad(pang)) * ((nH - tickH) / 2)
        return (
          <rect
            x={cxn - (tickW + 1) / 2}
            y={cyn - nH / 2}
            width={tickW + 1}
            height={nH}
            rx={1.5}
            fill={dT >= redT ? colorRedline : colorPointer}
            fillOpacity={dOp}
            transform={`rotate(${pang + 90} ${cxn} ${cyn})`}
          />
        )
      })()}

      {/* labels outside the majors */}
      {majorCount >= 2 &&
        Array.from({ length: majorCount }, (_, m) => {
          const t = m / (majorCount - 1)
          const lp = arcAt(radius, gapDeg, t)
          const off = majorH / 2 + labelSize * 0.9
          const { px, py, pang } = place(lp.x, lp.y, lp.ang)
          const lx = px + Math.cos(rad(pang)) * off
          const ly = py + Math.sin(rad(pang)) * off
          return (
            <text
              key={`l${m}`}
              x={lx}
              y={ly}
              fill={colorMajor}
              fontSize={labelSize}
              textAnchor="middle"
              dominantBaseline="central"
            >
              {labels[m] ?? ''}
            </text>
          )
        })}
    </svg>
  )
}
