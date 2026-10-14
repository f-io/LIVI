export const BASE_W = 1280
export const BASE_H = 720

export const CENTER_X = Math.round(BASE_W / 2)

// ── Two-ring layout: speed gauge (left) + mirrored RPM gauge (right), free centre ──
// Each gauge is a 180° half-circle (round outer edge, open toward the centre) capped by a few
// symmetric horizontal ticks. A minimalist tick scale with labelled majors and a glowing
// pointer that runs along the arc. Geometry in the gauge's own SVG units.
export const GAUGE_RADIUS = 110
export const GAUGE_GAP_DEG = 180
export const GAUGE_ARM_TICKS = 3

export const RING_W = 470
export const RING_H = 600
export const RING_TOP = 60
export const LEFT_RING_LEFT = -32
export const RIGHT_RING_LEFT = BASE_W - RING_W + 40

// Nudge each readout off the gauge centre toward the screen centre (speed right, gear left).
export const READOUT_DX = 34

export const NAV_X = CENTER_X
export const NAV_Y = 460

export const NAV_DIVIDER_Y = 532

// Full-map cluster dashboard. `ring` is the soft elliptical backdrop behind each gauge,
// centred on the gauge box so the left/right shadows are mirror-symmetric and the 0/P readout stays
// legible over the map. soft/end = % radius where the glow is still solid / fully faded.
export const VIGNETTE = {
  ring: {
    alpha: 0.85,
    soft: 44,
    end: 84,
    shape: 'ellipse 58% 56%'
  }
}

// Oil-temp (left) + fuel/charge (right) sit in one bottom bar
export const GAUGE_BAR_TOP = 672
export const GAUGE_BAR_W = 1000
export const FUEL_SEGMENTS = 8
