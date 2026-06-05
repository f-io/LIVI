import { Box, useTheme } from '@mui/material'
import { CarType } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useEffect, useRef, useState } from 'react'
import { DashShell } from '../../components/DashShell'
import { useVehicleTelemetry } from '../../hooks/useVehicleTelemetry'
import {
  FuelGauge,
  GaugeArc,
  NavMini,
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
  MAX_SPEED_KPH,
  NAV_Y,
  READOUT_DX,
  RIGHT_RING_LEFT,
  RING_H,
  RING_TOP,
  RING_W,
  RPM_LABELS,
  RPM_REDLINE,
  RPM_SCALE_MAX,
  SPEED_LABELS,
  SPEED_SCALE_MAX
} from './constants'

const clamp = (v: number, lo: number, hi: number): number => Math.max(lo, Math.min(hi, v))

export function Dash1() {
  const theme = useTheme()
  const { telemetry } = useVehicleTelemetry()

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

  return (
    <DashShell>
      <Box
        ref={hostRef}
        sx={{
          width: '100%',
          height: '100%',
          position: 'relative',
          overflow: 'hidden'
        }}
      >
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
          {/* LEFT RING — speed (arc fills with km/h, no redline) */}
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

          {/* RIGHT RING — RPM (same arc, mirrored so it opens toward the centre) */}
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

          {/* TELLTALE BAR — full-width top strip: turn arrows on the outer edges. */}
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

          {/* NAV MINI — turn-by-turn at its original lower-centre position */}
          <Box
            sx={{
              position: 'absolute',
              left: CENTER_X,
              top: NAV_Y,
              transform: 'translate(-50%, -50%)',
              width: 220,
              height: 120,
              display: 'grid',
              placeItems: 'center'
            }}
          >
            <NavMini iconSize={84} />
          </Box>

          {/* BOTTOM BAR — oil temp (left) + fuel/charge (right), pushed to the sides so the
              centre column stays free for the cluster stream. */}
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
