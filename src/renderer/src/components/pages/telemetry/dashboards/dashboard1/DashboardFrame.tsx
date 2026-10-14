import { Box, useTheme } from '@mui/material'
import { speedIn } from '@renderer/utils/units'
import { CarType, DEFAULT_CONFIG } from '@shared/types'
import { useLiviStore, useStatusStore } from '@store/store'
import { type ReactNode, useEffect, useMemo, useState } from 'react'
import { DashboardShell } from '../../components/DashboardShell'
import { useVehicleTelemetry } from '../../hooks/useVehicleTelemetry'
import {
  FuelGauge,
  GaugeArc,
  labelOverflow,
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
  GAUGE_RADIUS,
  LEFT_RING_LEFT,
  READOUT_DX,
  RIGHT_RING_LEFT,
  RING_H,
  RING_TOP,
  RING_W
} from '../constants'
import { gaugeScale, RPM_MAX_SECTIONS, RPM_STEP, SPEED_MAX_SECTIONS } from '../gaugeScale'

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v))

const stageScale = (w: number, h: number): number => {
  const s = Math.min(w / BASE_W, h / BASE_H)
  return Number.isFinite(s) && s > 0 ? s : 1
}

const stagePush = (w: number, safe: number): number => Math.max(0, (w / safe - BASE_W) / 2)

const stageOffset = (w: number, h: number): { left: number; top: number } => {
  const safe = stageScale(w, h)
  return {
    left: Math.round((w - BASE_W * safe) / 2),
    top: Math.round((h - BASE_H * safe) / 2)
  }
}

export type DashboardFrameProps = {
  /** Centre slot, e.g. the mini-nav or the full nav. */
  children?: ReactNode
  /** The cluster map fills the whole background and each instrument floats over it; a soft
      elliptical backdrop behind each gauge keeps the readouts legible over the map. */
  clusterFull?: boolean
}

/**
 * Shared dashboard frame: speed + gear gauges, telltale bar, oil/fuel bottom bar on a scaled
 * 1280×720 stage over a dark backdrop. The centre is a slot (pass mini-nav or full nav as
 * `children`); set `clusterFull` to drop the backdrop so the cluster plane shows behind.
 */
export function DashboardFrame({ children, clusterFull }: DashboardFrameProps) {
  const theme = useTheme()
  const { telemetry } = useVehicleTelemetry()
  const isClusterDashboard = clusterFull === true
  const setClusterDashboardActive = useStatusStore((s) => s.setClusterDashboardActive)
  useEffect(() => {
    if (!isClusterDashboard) return
    setClusterDashboardActive(true)
    return () => setClusterDashboardActive(false)
  }, [isClusterDashboard, setClusterDashboardActive])

  const speedKph = typeof telemetry?.speedKph === 'number' ? telemetry.speedKph : 0
  const rpm = typeof telemetry?.rpm === 'number' ? telemetry.rpm : 0
  const gear: string | number = telemetry?.gear ?? 'P'

  const turn =
    telemetry?.turnSignal === 'left' || telemetry?.turnSignal === 'right'
      ? telemetry.turnSignal
      : 'none'
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
  const speedUnit = useLiviStore((s) => s.settings?.speedUnit ?? DEFAULT_CONFIG.speedUnit)
  const temperatureUnit = useLiviStore(
    (s) => s.settings?.temperatureUnit ?? DEFAULT_CONFIG.temperatureUnit
  )
  const maxSpeedKph = useLiviStore((s) => s.settings?.maxSpeedKph ?? DEFAULT_CONFIG.maxSpeedKph)
  const speedStep = useLiviStore((s) => s.settings?.speedScaleStep ?? DEFAULT_CONFIG.speedScaleStep)
  const maxRpm = useLiviStore((s) => s.settings?.maxRpm ?? DEFAULT_CONFIG.maxRpm)
  const redlineRpm = useLiviStore((s) => s.settings?.redlineRpm ?? DEFAULT_CONFIG.redlineRpm)
  const speedScale = useMemo(
    () => gaugeScale(speedIn(maxSpeedKph, speedUnit), speedStep, SPEED_MAX_SECTIONS, String),
    [maxSpeedKph, speedUnit, speedStep]
  )
  const rpmScale = useMemo(
    () => gaugeScale(maxRpm, RPM_STEP, RPM_MAX_SECTIONS, (v) => String(v / 1000)),
    [maxRpm]
  )
  // Both rings move in together, as far as the number nearest the screen edge needs.
  const ringShift = useMemo(() => {
    const gauge = { radius: GAUGE_RADIUS, gapDeg: GAUGE_GAP_DEG, armTicks: GAUGE_ARM_TICKS }
    return Math.max(
      labelOverflow(
        { ...gauge, ...speedScale },
        { boxW: RING_W, boxH: RING_H, px: -LEFT_RING_LEFT }
      ),
      labelOverflow(
        { ...gauge, ...rpmScale },
        { boxW: RING_W, boxH: RING_H, px: RIGHT_RING_LEFT + RING_W - BASE_W }
      )
    )
  }, [speedScale, rpmScale])
  const speed = speedIn(speedKph, speedUnit)

  const [scale, setScale] = useState(() => stageScale(window.innerWidth, window.innerHeight))
  const [sidePush, setSidePush] = useState(() =>
    stagePush(window.innerWidth, stageScale(window.innerWidth, window.innerHeight))
  )
  const [offset, setOffset] = useState(() => stageOffset(window.innerWidth, window.innerHeight))
  const [scaleLive, setScaleLive] = useState(false)

  // Single source of truth: the dashboard fills the fixed full-window telemetry root, so
  // the window size IS the host size. It is read once at first render and again only
  // on window resize / fullscreen transitions — never re-measured in between.
  useEffect(() => {
    let settle: ReturnType<typeof setTimeout> | null = null
    // Debounced: attaching the native cluster plane makes macOS relayout the window,
    // which fires transient 1-2px resize blips. Only a size that holds for a beat is
    // a real resize/fullscreen change; a blip collapses to a no-op.
    const onResize = () => {
      if (settle != null) clearTimeout(settle)
      settle = setTimeout(() => {
        settle = null
        const w = window.innerWidth
        const h = window.innerHeight
        const safe = stageScale(w, h)
        setScale(safe)
        setSidePush(stagePush(w, safe))
        setOffset(stageOffset(w, h))
      }, 150)
    }
    window.addEventListener('resize', onResize)
    return () => {
      if (settle != null) clearTimeout(settle)
      window.removeEventListener('resize', onResize)
    }
  }, [])

  useEffect(() => {
    setScaleLive(true)
  }, [])

  const clusterBg = theme.palette.background.default
  const isDark = theme.palette.mode === 'dark'
  const calShadowColor = isDark ? 'rgba(0, 0, 0, 0.55)' : 'rgba(255, 255, 255, 0.72)'
  const calRectColor = isDark ? 'rgba(0, 0, 0, 0.6)' : 'rgba(255, 255, 255, 0.78)'

  return (
    <DashboardShell>
      <Box sx={{ width: '100%', height: '100%', position: 'relative', overflow: 'hidden' }}>
        {/* Normal cluster: a plain dark backdrop. Either cluster mode drops it so the plane shows. */}
        {!isClusterDashboard && (
          <Box sx={{ position: 'absolute', inset: 0, backgroundColor: clusterBg }} />
        )}

        {/* scaled stage */}
        <Box
          sx={{
            position: 'absolute',
            left: offset.left,
            top: offset.top,
            width: BASE_W,
            height: BASE_H,
            transform: `scale(${scale})`,
            transformOrigin: 'top left',
            willChange: 'transform',
            transition: scaleLive ? 'transform 0.05s ease-out' : 'none'
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
              transform: `translateX(${ringShift - sidePush}px)`
            }}
          >
            <Box sx={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
              <GaugeArc
                value={speed}
                scaleMax={speedScale.scaleMax}
                ticks={speedScale.ticks}
                radius={GAUGE_RADIUS}
                gapDeg={GAUGE_GAP_DEG}
                armTicks={GAUGE_ARM_TICKS}
                majorCount={speedScale.majorCount}
                labels={speedScale.labels}
                colorScale={theme.palette.text.disabled}
                colorMajor={theme.palette.text.secondary}
                colorPointer={theme.palette.text.primary}
                colorRedline={theme.palette.error.main}
                shadow={clusterFull}
                shadowColor={calShadowColor}
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
                value={clamp(Math.round(speed), 0, 999)}
                label={speedUnit === 'mph' ? 'MPH' : 'KPH'}
                align="end"
                maxChars={3}
                backdropColor={clusterFull ? calRectColor : undefined}
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
              transform: `translateX(${sidePush - ringShift}px)`
            }}
          >
            <Box sx={{ position: 'absolute', inset: 0, pointerEvents: 'none' }}>
              <GaugeArc
                value={rpm}
                scaleMax={rpmScale.scaleMax}
                redline={redlineRpm}
                ticks={rpmScale.ticks}
                radius={GAUGE_RADIUS}
                gapDeg={GAUGE_GAP_DEG}
                armTicks={GAUGE_ARM_TICKS}
                majorCount={rpmScale.majorCount}
                labels={rpmScale.labels}
                mirror
                colorScale={theme.palette.text.disabled}
                colorMajor={theme.palette.text.secondary}
                colorPointer={theme.palette.text.primary}
                colorRedline={theme.palette.error.main}
                shadow={clusterFull}
                shadowColor={calShadowColor}
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
              <SoftReadout
                value={normalizeGear(gear)}
                label="GEAR"
                align="start"
                maxChars={3}
                backdropColor={clusterFull ? calRectColor : undefined}
              />
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
              temperatureUnit={temperatureUnit}
              size={30}
            />
          </Box>

          {/* CENTRE SLOT — mini-nav (Dashboard 1), full nav (Dashboard 2), or empty (Dashboard 3 cluster) */}
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
            <TempGauge value={oilC} unit={temperatureUnit} segments={FUEL_SEGMENTS} />
            <FuelGauge level={fuelPct} mode={fuelMode} segments={FUEL_SEGMENTS} />
          </Box>
        </Box>
      </Box>
    </DashboardShell>
  )
}
