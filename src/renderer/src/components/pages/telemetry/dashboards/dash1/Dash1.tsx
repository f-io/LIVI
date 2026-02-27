import { useEffect, useRef, useState } from 'react'
import { Box, useTheme } from '@mui/material'
import { DashShell } from '../../components/DashShell'
import { useVehicleTelemetry } from '../../hooks/useVehicleTelemetry'
import { Speed, Rpm, RpmRing, Gear, CoolantTemp, OilTemp, FuelLevel, NavMini } from '../../widgets'
import {
  BASE_W,
  BASE_H,
  SPEED_GROUP_LEFT,
  SPEED_GROUP_TOP,
  SPEED_GROUP_W,
  NAV_X,
  NAV_Y,
  GEAR_X,
  GEAR_Y,
  RPM_RIGHT,
  RPM_TOP,
  METRICS_RIGHT,
  METRICS_TOP
} from './constants'

export function Dash1() {
  const theme = useTheme()
  const { telemetry } = useVehicleTelemetry()

  const speedKph = typeof telemetry?.speedKph === 'number' ? telemetry.speedKph : 0
  const rpm = typeof telemetry?.rpm === 'number' ? telemetry.rpm : 0
  const coolantC = typeof telemetry?.coolantC === 'number' ? telemetry.coolantC : 0
  const oilC = typeof telemetry?.oilC === 'number' ? telemetry.oilC : 0
  const fuelPct = typeof telemetry?.fuelPct === 'number' ? telemetry.fuelPct : 0

  const gear: string | number = telemetry?.gear ?? 'P'

  const hostRef = useRef<HTMLDivElement | null>(null)
  const [scale, setScale] = useState(1)

  useEffect(() => {
    const el = hostRef.current
    if (!el) return

    const ro = new ResizeObserver(([entry]) => {
      const r = entry?.contentRect
      if (!r) return
      const s = Math.min(r.width / BASE_W, r.height / BASE_H)
      setScale(Number.isFinite(s) && s > 0 ? s : 1)
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
          {/* SPEED + RING GROUP */}
          <Box
            sx={{
              position: 'absolute',
              left: SPEED_GROUP_LEFT,
              top: SPEED_GROUP_TOP,
              width: SPEED_GROUP_W,
              height: 640
            }}
          >
            <Box
              sx={{
                position: 'absolute',
                inset: 0,
                pointerEvents: 'none',
                transform: 'translate(-36px, -22px)',
                transformOrigin: 'center'
              }}
            >
              <RpmRing
                rpm={rpm}
                maxRpm={5000}
                redlineRpm={4500}
                ticks={44}
                startDeg={90}
                arcDeg={240}
                colorOff={theme.palette.text.disabled}
                colorOn={theme.palette.text.primary}
                colorRedline={theme.palette.error.main}
              />
            </Box>

            <Box
              sx={{
                position: 'absolute',
                left: '50%',
                top: '50%',
                transform: 'translate(-50%, -50%)'
              }}
            >
              <Speed speedKph={speedKph} />
            </Box>
          </Box>

          {/* NAV MINI — centered on stage */}
          <Box
            sx={{
              position: 'absolute',
              left: NAV_X,
              top: NAV_Y,
              transform: 'translate(-50%, -50%) scale(1.35)',
              width: 220,
              height: 120,
              display: 'grid',
              placeItems: 'center'
            }}
          >
            <NavMini iconSize={64} />
          </Box>

          {/* GEAR */}
          <Box
            sx={{
              position: 'absolute',
              left: GEAR_X,
              top: GEAR_Y,
              transform: 'translate(-50%, -50%)',
              width: 110,
              height: 120,
              display: 'grid',
              placeItems: 'center'
            }}
          >
            <Gear gear={gear} />
          </Box>

          {/* RPM */}
          <Box sx={{ position: 'absolute', right: RPM_RIGHT, top: RPM_TOP }}>
            <Rpm rpm={rpm} />
          </Box>

          {/* RIGHT METRICS COLUMN (coolant / oil / fuel) */}
          <Box
            sx={{
              position: 'absolute',
              right: METRICS_RIGHT,
              top: METRICS_TOP,
              width: 'fit-content',
              display: 'grid',
              rowGap: 1.9,
              justifyItems: 'end',
              transform: 'scale(1.45)',
              transformOrigin: 'right top'
            }}
          >
            <CoolantTemp coolantC={coolantC} />
            <OilTemp oilC={oilC} />
            <FuelLevel fuelPct={fuelPct} />
          </Box>
        </Box>
      </Box>
    </DashShell>
  )
}
