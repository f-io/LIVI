// Standard ISO-2575 style telltale glyphs

function Lamp({ rayDrop }: { rayDrop: number }) {
  return (
    <>
      <path d="M15 5 A7 7 0 0 0 15 19 Z" fill="currentColor" />
      {[8, 11, 14, 17].map((y) => (
        <line
          key={y}
          x1="1"
          y1={y + rayDrop}
          x2="6.5"
          y2={y}
          stroke="currentColor"
          strokeWidth="1.7"
          strokeLinecap="round"
        />
      ))}
    </>
  )
}

export function HighBeamGlyph() {
  return (
    <svg viewBox="0 0 24 24" width="100%" height="100%" aria-hidden="true">
      <Lamp rayDrop={0} />
    </svg>
  )
}

export function LowBeamGlyph() {
  return (
    <svg viewBox="0 0 24 24" width="100%" height="100%" aria-hidden="true">
      <Lamp rayDrop={2.5} />
    </svg>
  )
}

export function ParkingBrakeGlyph() {
  // "(P)"
  return (
    <svg viewBox="0 0 24 24" width="100%" height="100%" aria-hidden="true">
      <circle cx="12" cy="12" r="6.4" fill="none" stroke="currentColor" strokeWidth="1.8" />
      <text
        x="12.5"
        y="15.6"
        textAnchor="middle"
        fontSize="9.5"
        fontWeight="700"
        fill="currentColor"
      >
        P
      </text>
      <path
        d="M3.5 8 A9 9 0 0 0 3.5 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
      <path
        d="M20.5 8 A9 9 0 0 1 20.5 16"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
      />
    </svg>
  )
}

export function HazardGlyph() {
  // The hazard-warning triangle
  return (
    <svg viewBox="0 0 24 24" width="100%" height="100%" aria-hidden="true">
      <path
        d="M12 4 L21 19 H3 Z"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinejoin="round"
      />
    </svg>
  )
}
