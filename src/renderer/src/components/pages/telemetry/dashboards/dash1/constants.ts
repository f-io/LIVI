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
export const GAUGE_TICKS = 41
export const GAUGE_MAJOR_COUNT = 6

// Speed scale (left): 0…200 km/h, labelled every 40.
export const SPEED_SCALE_MAX = 200
export const SPEED_LABELS = ['0', '40', '80', '120', '160', '200']

// RPM scale (right): 0…5000 rpm in thousands, redline at 4500.
export const RPM_SCALE_MAX = 5000
export const RPM_REDLINE = 4500
export const RPM_LABELS = ['0', '1', '2', '3', '4', '5']

export const RING_W = 470
export const RING_H = 600
export const RING_TOP = 40
export const LEFT_RING_LEFT = -32
export const RIGHT_RING_LEFT = BASE_W - RING_W + 40
export const MAX_SPEED_KPH = 220

// Nudge each readout off the gauge centre toward the screen centre (speed right, gear left).
export const READOUT_DX = 34

export const NAV_X = CENTER_X
export const NAV_Y = 460

// Oil-temp (left) + fuel/charge (right) sit in one bottom bar, pushed to the sides so the centre
// column stays free for the cluster stream (only the mini-nav lives in the inner area).
export const GAUGE_BAR_TOP = 672
export const GAUGE_BAR_W = 1000
export const FUEL_SEGMENTS = 8
