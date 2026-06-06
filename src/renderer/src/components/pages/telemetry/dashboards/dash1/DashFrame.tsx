import { Box, useTheme } from '@mui/material'
import { CarType } from '@shared/types'
import { useLiviStore, useStatusStore } from '@store/store'
import { type ReactNode, useEffect, useRef, useState } from 'react'
import { DashShell } from '../../components/DashShell'
import { useVehicleTelemetry } from '../../hooks/useVehicleTelemetry'
import {
  FuelGauge,
  GaugeArc,
  normalizeGear,
  SoftReadout,
  TelltaleBar,
  TempGauge
} from '../../widgets'
import {
  BASE_H,
  BASE_W,
  CENTER_X,
  FUEL_SEGMENTS,
  GAUGE_ARM_TICKS,
  GAUGE_BAR_TOP,
  GAUGE_BAR_W,
  GAUGE_GAP_DEG,
  GAUGE_MAJOR_COUNT,
  GAUGE_RADIUS,
  GAUGE_TICKS,
  LEFT_RING_LEFT,
  READOUT_DX,
  RIGHT_RING_LEFT,
  RING_H,
  RING_TOP,
  RING_W,
  RPM_LABELS,
  RPM_REDLINE,
  RPM_SCALE_MAX,
  SPEED_LABELS,
  SPEED_SCALE_MAX,
  VIGNETTE
} from '../constants'
import { SoftPanel } from './SoftPanel'

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v))

export type DashFrameProps = {
  /** Centre slot, e.g. the mini-nav or the full nav. */
  children?: ReactNode
  /** The cluster map fills the whole background and each instrument floats over it; theme-aware
      vignette bands top/bottom keep the telltale row and bottom bar legible. */
  clusterFull?: boolean
}

/**
 * Shared dash frame: speed + gear gauges, telltale bar, oil/fuel bottom bar on a scaled
 * 1280×720 stage over a dark backdrop. The centre is a slot (pass mini-nav or full nav as
 * `children`); set `clusterFull` to drop the backdrop so the cluster plane shows behind.
 */
export function DashFrame({ children, clusterFull }: DashFrameProps) {
  const theme = useTheme()
  const { telemetry } = useVehicleTelemetry()

  // Either cluster mode reveals the full-window plane via the single App-level <Cluster> overlay:
  // flag this dash active on mount, clear on unmount.
  const isClusterDash = clusterFull === true
  const setClusterDashActive = useStatusStore((s) => s.setClusterDashActive)
  useEffect(() => {
    if (!isClusterDash) return
    setClusterDashActive(true)
    return () => setClusterDashActive(false)
  }, [isClusterDash, setClusterDashActive])

  const speedKph = typeof telemetry?.speedKph === 'number' ? telemetry.speedKph : 0
  const rpm = typeof telemetry?.rpm === 'number' ? telemetry.rpm : 0
  const gear: string | number = telemetry?.gear ?? 'P'

  const turn = telemetry?.turn === 'left' || telemetry?.turn === 'right' ? telemetry.turn : 'none'
  const hazards = telemetry?.hazards === true
  const lights = telemetry?.lights === true
  const highBeam = telemetry?.highBeam === true
  const parkingBrake = telemetry?.parkingBrake === true
  const ambientC = typeof telemetry?.ambientC === 'number' ? telemetry.ambientC : undefined
  const fuelPct = typeof telemetry?.fuelPct === 'number' ? telemetry.fuelPct : 0
  const oilC = typeof telemetry?.oilC === 'number' ? telemetry.oilC : 0

  // Battery vs fuel icon, driven by the configured car type (controllable in settings).
  const carType = useLiviStore((s) => s.settings?.carType)
  const fuelMode: 'fuel' | 'battery' = carType === CarType.Electric ? 'battery' : 'fuel'

  const hostRef = useRef<HTMLDivElement | null>(null)
  const [scale, setScale] = useState(1)
  const [sidePush, setSidePush] = useState(0)

  useEffect(() => {
    const el = hostRef.current
    if (!el) return

    const ro = new ResizeObserver(([entry]) => {
      const r = entry?.contentRect
      if (!r) return
      const s = Math.min(r.width / BASE_W, r.height / BASE_H)
      const safe = Number.isFinite(s) && s > 0 ? s : 1
      setScale(safe)
      setSidePush(Math.max(0, (r.width / safe - BASE_W) / 2))
    })

    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const clusterBg = theme.palette.background.default
  // Vignette bands: dark in dark mode, white in light mode (matches SoftPanel).
  const bandRgb = theme.palette.mode === 'light' ? '255, 255, 255' : '0, 0, 0'
  // Eased fade so the inner edge melts into the map instead of a hard linear cut: the band stays
  // solid near the screen edge (covers the telltale row / temp-fuel bar) then tails off gently.
  const bandFade = (dir: 'to top' | 'to bottom'): string => {
    const a = VIGNETTE.bandAlpha
    return `linear-gradient(${dir}, rgba(${bandRgb},${a}) 0%, rgba(${bandRgb},${a * 0.55}) 38%, rgba(${bandRgb},${a * 0.16}) 72%, rgba(${bandRgb},0) 100%)`
  }

  return (
    <DashShell>
      <Box
        ref={hostRef}
        sx={{ width: '100%', height: '100%', position: 'relative', overflow: 'hidden' }}
      >
        {/* Normal dash: a plain dark backdrop. Either cluster mode drops it so the plane shows. */}
        {!isClusterDash && (
          <Box sx={{ position: 'absolute', inset: 0, backgroundColor: clusterBg }} />
        )}

        {/* clusterFull: full-window map behind, soft theme-aware bands top + bottom so the telltale
            row and the bottom bar stay legible. Host-space (full width), behind the scaled stage. */}
        {clusterFull && (
          <>
            <Box
              sx={{
                position: 'absolute',
                top: 0,
                left: 0,
                right: 0,
                height: `${VIGNETTE.bandTopPct}%`,
                pointerEvents: 'none',
                background: bandFade('to bottom')
              }}
            />
            <Box
              sx={{
                position: 'absolute',
                bottom: 0,
                left: 0,
                right: 0,
                height: `${VIGNETTE.bandBottomPct}%`,
                pointerEvents: 'none',
                background: bandFade('to top')
              }}
            />
          </>
        )}

        {/* scaled stage */}
        <Box
          sx={{
            position: 'absolute',
            left: '50%',
            top: '50%',
            width: BASE_W,
            height: BASE_H,
            transform: `translate(-50%, -50%) scale(${scale})`,
            transformOrigin: 'center',
            transition: 'transform 0.05s ease-out'
          }}
        >
          {/* LEFT RING — speed */}
          <Box
            sx={{
              position: 'absolute',
              left: LEFT_RING_LEFT,
              top: RING_TOP,
              width: RING_W,
              height: RING_H,
              transform: `translateX(${-sidePush}px)`
            }}
          >
            {clusterFull && <SoftPanel {...VIGNETTE.ring} />}
            <Box sx={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
              <GaugeArc
                value={speedKph}
                scaleMax={SPEED_SCALE_MAX}
                ticks={GAUGE_TICKS}
                radius={GAUGE_RADIUS}
                gapDeg={GAUGE_GAP_DEG}
                armTicks={GAUGE_ARM_TICKS}
                majorCount={GAUGE_MAJOR_COUNT}
                labels={SPEED_LABELS}
                colorScale={theme.palette.text.disabled}
                colorMajor={theme.palette.text.secondary}
                colorPointer={theme.palette.text.primary}
                colorRedline={theme.palette.error.main}
              />
            </Box>
            <Box
              sx={{
                position: 'absolute',
                left: '50%',
                top: '50%',
                width: 200,
                height: 130,
                transform: `translate(calc(-50% + ${READOUT_DX}px), -50%)`
              }}
            >
              <SoftReadout
                value={clamp(Math.round(speedKph), 0, 999)}
                label="KPH"
                align="end"
                maxChars={3}
              />
            </Box>
          </Box>

          {/* RIGHT RING — RPM (mirrored so it opens toward the centre) */}
          <Box
            sx={{
              position: 'absolute',
              left: RIGHT_RING_LEFT,
              top: RING_TOP,
              width: RING_W,
              height: RING_H,
              transform: `translateX(${sidePush}px)`
            }}
          >
            {clusterFull && <SoftPanel {...VIGNETTE.ring} />}
            <Box sx={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
              <GaugeArc
                value={rpm}
                scaleMax={RPM_SCALE_MAX}
                redline={RPM_REDLINE}
                ticks={GAUGE_TICKS}
                radius={GAUGE_RADIUS}
                gapDeg={GAUGE_GAP_DEG}
                armTicks={GAUGE_ARM_TICKS}
                majorCount={GAUGE_MAJOR_COUNT}
                labels={RPM_LABELS}
                mirror
                colorScale={theme.palette.text.disabled}
                colorMajor={theme.palette.text.secondary}
                colorPointer={theme.palette.text.primary}
                colorRedline={theme.palette.error.main}
              />
            </Box>
            <Box
              sx={{
                position: 'absolute',
                left: '50%',
                top: '50%',
                width: 200,
                height: 130,
                transform: `translate(calc(-50% - ${READOUT_DX}px), -50%)`
              }}
            >
              <SoftReadout value={normalizeGear(gear)} label="GEAR" align="start" maxChars={3} />
            </Box>
          </Box>

          {/* TELLTALE BAR */}
          <Box
            sx={{
              position: 'absolute',
              left: CENTER_X,
              top: 12,
              transform: 'translateX(-50%)',
              width: 1140
            }}
          >
            <TelltaleBar
              lights={lights}
              highBeam={highBeam}
              parkingBrake={parkingBrake}
              turn={turn}
              hazards={hazards}
              ambientC={ambientC}
              size={30}
            />
          </Box>

          {/* CENTRE SLOT — mini-nav (Dash 1), full nav (Dash 2), or empty (Dash 3 cluster) */}
          {children}

          {/* BOTTOM BAR — oil temp (left) + fuel/charge (right) */}
          <Box
            sx={{
              position: 'absolute',
              left: CENTER_X,
              top: GAUGE_BAR_TOP,
              transform: 'translateX(-50%)',
              width: GAUGE_BAR_W,
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center'
            }}
          >
            <TempGauge value={oilC} segments={FUEL_SEGMENTS} />
            <FuelGauge level={fuelPct} mode={fuelMode} segments={FUEL_SEGMENTS} />
          </Box>
        </Box>
      </Box>
    </DashShell>
  )
}
