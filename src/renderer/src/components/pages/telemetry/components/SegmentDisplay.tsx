import type { CSSProperties } from 'react'

export type SegmentDisplayProps = {
  value: string | number
  digits: number
  onColor?: string
  offColor?: string
  offMode?: 'dim' | 'blank'
  dimLeadingZeros?: boolean
  leadingZeroColor?: string
  className?: string
  style?: CSSProperties
}

type SegIndex = 0 | 1 | 2 | 3 | 4 | 5 | 6
// 0=A (top), 1=B (upper-right), 2=C (lower-right), 3=D (bottom), 4=E (lower-left), 5=F (upper-left), 6=G (middle)

const DIGIT_H = 100
const DIGIT_W = 60
const DIGIT_GAP = 7 // tune

const MARGIN = 9 // tune
const THICK = 10 // tune
const PHASE_GAP = 2 // tune
const TIP_ANGLE_DEG = 45 // tune
const MAX_BEVEL = 12 // tune

const segMask = (...on: SegIndex[]) => {
  let m = 0
  for (const i of on) m |= 1 << i
  return m
}

const SEGMENT_MASK: Record<string, number> = {
  ' ': 0,
  '-': segMask(6),

  '0': segMask(0, 1, 2, 3, 4, 5),
  '1': segMask(1, 2),
  '2': segMask(0, 1, 6, 4, 3),
  '3': segMask(0, 1, 6, 2, 3),
  '4': segMask(5, 6, 1, 2),
  '5': segMask(0, 5, 6, 2, 3),
  '6': segMask(0, 5, 6, 2, 3, 4),
  '7': segMask(0, 1, 2),
  '8': segMask(0, 1, 2, 3, 4, 5, 6),
  '9': segMask(0, 1, 2, 3, 5, 6)
}

function clampChar(c: string) {
  const up = c.toUpperCase()
  if (up.length !== 1) return ' '
  return SEGMENT_MASK[up] != null ? up : ' '
}

export function SegmentDisplay({
  value,
  digits,
  onColor = 'currentColor',
  offColor = 'rgba(255,255,255,0.10)',
  offMode = 'blank',
  dimLeadingZeros = false,
  leadingZeroColor = 'rgba(255,255,255,0.22)',
  className,
  style
}: SegmentDisplayProps) {
  const raw = String(value)

  // only allow digits, space, minus; everything else becomes space
  const sanitized = raw
    .split('')
    .map((c) => (SEGMENT_MASK[c.toUpperCase()] != null ? c : ' '))
    .join('')

  const str =
    sanitized.length >= digits
      ? sanitized.slice(-digits)
      : ' '.repeat(digits - sanitized.length) + sanitized

  const padded = str.split('').map(clampChar).join('')

  const totalWidth = digits * DIGIT_W + (digits - 1) * DIGIT_GAP
  let seenNonZero = false

  return (
    <svg
      className={className}
      style={style}
      viewBox={`0 0 ${totalWidth} ${DIGIT_H}`}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="7-segment display"
    >
      {padded.split('').map((char, i) => {
        const isNumeric = char >= '0' && char <= '9'
        const isLeadingZero =
          dimLeadingZeros && isNumeric && !seenNonZero && char === '0' && i < padded.length - 1

        if (isNumeric && char !== '0') seenNonZero = true
        if (char !== '0' && char !== '-' && char !== ' ') seenNonZero = true

        return (
          <g key={i} transform={`translate(${i * (DIGIT_W + DIGIT_GAP)}, 0)`}>
            <Digit
              char={char}
              onColor={isLeadingZero ? leadingZeroColor : onColor}
              offColor={offColor}
              offMode={offMode}
            />
          </g>
        )
      })}
    </svg>
  )
}

function Digit({
  char,
  onColor,
  offColor,
  offMode
}: {
  char: string
  onColor: string
  offColor: string
  offMode: 'dim' | 'blank'
}) {
  const mask = SEGMENT_MASK[char] ?? 0
  const blankDigit = char === ' ' && offMode === 'blank'

  const fillFor = (on: boolean) => {
    if (on) return onColor
    return blankDigit ? 'transparent' : offColor
  }

  const segOn = (i: SegIndex) => (mask & (1 << i)) !== 0
  const poly = (pts: Array<[number, number]>) => pts.map(([x, y]) => `${x},${y}`).join(' ')

  const tipGap = PHASE_GAP / 2

  const xL = MARGIN
  const xR = DIGIT_W - MARGIN
  const yT = MARGIN
  const yB = DIGIT_H - MARGIN
  const yM = DIGIT_H / 2

  const LT: [number, number] = [xL, yT]
  const RT: [number, number] = [xR, yT]
  const LM: [number, number] = [xL, yM]
  const RM: [number, number] = [xR, yM]
  const LB: [number, number] = [xL, yB]
  const RB: [number, number] = [xR, yB]

  const beveledSegment = (a: [number, number], b: [number, number]) => {
    let [ax, ay] = a
    let [bx, by] = b

    // phase gap: shorten on both ends
    const dx0 = bx - ax
    const dy0 = by - ay
    const len0 = Math.hypot(dx0, dy0) || 1
    const ux0 = dx0 / len0
    const uy0 = dy0 / len0
    ax += ux0 * tipGap
    ay += uy0 * tipGap
    bx -= ux0 * tipGap
    by -= uy0 * tipGap

    const dx = bx - ax
    const dy = by - ay
    const len = Math.hypot(dx, dy) || 1
    const ux = dx / len
    const uy = dy / len
    const px = -uy
    const py = ux

    const half = THICK / 2
    const ang = (TIP_ANGLE_DEG * Math.PI) / 180
    const bevelFromAngle = half / Math.tan(ang)
    const bevel = Math.min(MAX_BEVEL, bevelFromAngle, len / 2)

    const ax1 = ax + ux * bevel
    const ay1 = ay + uy * bevel
    const bx1 = bx - ux * bevel
    const by1 = by - uy * bevel

    return poly([
      [ax, ay],
      [ax1 + px * half, ay1 + py * half],
      [bx1 + px * half, by1 + py * half],
      [bx, by],
      [bx1 - px * half, by1 - py * half],
      [ax1 - px * half, ay1 - py * half]
    ])
  }

  // 7 segments
  const A = beveledSegment(LT, RT)
  const B = beveledSegment(RT, RM)
  const C = beveledSegment(RM, RB)
  const D = beveledSegment(LB, RB)
  const E = beveledSegment(LM, LB)
  const F = beveledSegment(LT, LM)
  const G = beveledSegment(LM, RM)

  return (
    <>
      <polygon points={A} fill={fillFor(segOn(0))} />
      <polygon points={B} fill={fillFor(segOn(1))} />
      <polygon points={C} fill={fillFor(segOn(2))} />
      <polygon points={D} fill={fillFor(segOn(3))} />
      <polygon points={E} fill={fillFor(segOn(4))} />
      <polygon points={F} fill={fillFor(segOn(5))} />
      <polygon points={G} fill={fillFor(segOn(6))} />
    </>
  )
}
