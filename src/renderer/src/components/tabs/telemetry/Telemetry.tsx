import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Box,
  Paper,
  Typography,
  Fab,
  Drawer,
  FormControlLabel,
  Checkbox,
  MenuItem,
  Select,
  Stack,
  Divider
} from '@mui/material'
import { useTheme } from '@mui/material/styles'
import SettingsOutlinedIcon from '@mui/icons-material/SettingsOutlined'
import ReactECharts from 'echarts-for-react'

import {
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  closestCenter,
  type DragEndEvent
} from '@dnd-kit/core'
import { SortableContext, useSortable, arrayMove, rectSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'

import { useTelemetryStore } from '../../../store/telemetryStore'
import { JSX } from 'react/jsx-runtime'

const clamp01 = (v: number) => Math.max(0, Math.min(1, v))

const fmt = (value?: number | null, decimals = 0) => {
  if (value == null || Number.isNaN(value)) return '—'
  const m = Math.pow(10, decimals)
  return String(Math.round(value * m) / m)
}

const easeOutCubic = (t: number) => 1 - Math.pow(1 - t, 3)

type AnimatedNumberProps = {
  value?: number
  decimals?: number
  live: boolean
}

const AnimatedNumber = ({ value, decimals = 0, live }: AnimatedNumberProps) => {
  const raf = useRef<number | null>(null)
  const curRef = useRef<number>(0)
  const [display, setDisplay] = useState<number>(0)

  useEffect(() => {
    const v = value
    if (v == null || Number.isNaN(v)) return
    curRef.current = v
    setDisplay(v)
  }, [])

  useEffect(() => {
    const v = value
    if (v == null || Number.isNaN(v)) return

    if (!live) {
      curRef.current = v
      setDisplay(v)
      return
    }

    const start = performance.now()
    const duration = 240
    const from = curRef.current
    const to = v

    if (raf.current) cancelAnimationFrame(raf.current)

    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / duration)
      const e = easeOutCubic(t)
      const x = from + (to - from) * e
      curRef.current = x
      setDisplay(x)
      if (t < 1) raf.current = requestAnimationFrame(tick)
    }

    raf.current = requestAnimationFrame(tick)

    return () => {
      if (raf.current) cancelAnimationFrame(raf.current)
      raf.current = null
    }
  }, [value, live])

  return <>{fmt(display, decimals)}</>
}

type MetricTileProps = {
  label: string
  value?: number
  unit: string
  decimals?: number
  min?: number
  max?: number
  live: boolean
  series?: number[]
  accent?: 'primary' | 'info' | 'success' | 'warning' | 'error'
}

const MetricTile = ({
  label,
  value,
  unit,
  decimals = 0,
  min = 0,
  max = 100,
  live,
  series = [],
  accent = 'primary'
}: MetricTileProps) => {
  const theme = useTheme()
  const hasValue = value != null && !Number.isNaN(value)
  const t = hasValue ? clamp01(((value as number) - min) / (max - min)) : 0

  const accentColor = theme.palette[accent].main
  const muted = theme.palette.text.secondary

  const ringOption = useMemo(() => {
    return {
      animation: live,
      animationDurationUpdate: 260,
      animationEasingUpdate: 'cubicOut',
      series: [
        {
          type: 'pie',
          radius: ['78%', '92%'],
          center: ['84%', '46%'],
          silent: true,
          hoverAnimation: false,
          label: { show: false },
          emphasis: { disabled: true },
          data: [
            { value: t, itemStyle: { color: accentColor } },
            { value: 1 - t, itemStyle: { color: 'rgba(255,255,255,0.08)' } }
          ]
        }
      ]
    }
  }, [accentColor, live, t])

  const sparkOption = useMemo(() => {
    const data = series.length ? series.slice(-40) : []
    return {
      animation: live,
      animationDurationUpdate: 260,
      animationEasingUpdate: 'cubicOut',
      grid: { left: 0, right: 0, top: 6, bottom: 0 },
      xAxis: { type: 'category', show: false, data: data.map((_, i) => i) },
      yAxis: { type: 'value', show: false, min: 'dataMin', max: 'dataMax' },
      series: [
        {
          type: 'line',
          data,
          showSymbol: false,
          smooth: true,
          lineStyle: { width: 2, color: 'rgba(255,255,255,0.22)' },
          areaStyle: { opacity: 0.12 }
        }
      ]
    }
  }, [live, series])

  return (
    <Paper
      elevation={0}
      sx={{
        p: 1.5,
        borderRadius: 3,
        border: `1px solid ${theme.palette.divider}`,
        position: 'relative',
        overflow: 'hidden',
        background: `
          radial-gradient(900px 240px at 0% 0%, rgba(255,255,255,0.10), transparent 60%),
          radial-gradient(700px 220px at 100% 0%, rgba(255,255,255,0.06), transparent 55%),
          linear-gradient(180deg, rgba(255,255,255,0.06), rgba(255,255,255,0.02))
        `,
        opacity: live ? 1 : 0.86,
        minHeight: 132
      }}
    >
      <Box
        sx={{
          position: 'absolute',
          inset: -80,
          background: `radial-gradient(circle at 85% 40%, ${accentColor}33, transparent 55%)`,
          filter: 'blur(18px)',
          pointerEvents: 'none'
        }}
      />

      <Box sx={{ position: 'relative' }}>
        <Typography
          variant="caption"
          sx={{
            color: muted,
            letterSpacing: 1.4,
            textTransform: 'uppercase',
            fontWeight: 700
          }}
        >
          {label}
        </Typography>

        <Box sx={{ display: 'flex', alignItems: 'baseline', gap: 1, mt: 0.45 }}>
          <Typography
            variant="h3"
            sx={{
              fontWeight: 900,
              letterSpacing: -0.6,
              lineHeight: 1,
              fontVariantNumeric: 'tabular-nums'
            }}
          >
            <AnimatedNumber value={value} decimals={decimals} live={live} />
          </Typography>
          <Typography variant="subtitle2" sx={{ color: muted, fontWeight: 700 }}>
            {unit}
          </Typography>
        </Box>

        <Box sx={{ mt: 0.9, height: 32, opacity: 0.9 }}>
          <ReactECharts option={sparkOption} style={{ height: '100%', width: '100%' }} />
        </Box>

        <Box sx={{ position: 'absolute', right: -10, top: -10, width: 96, height: 96 }}>
          <ReactECharts option={ringOption} style={{ height: '100%', width: '100%' }} />
        </Box>
      </Box>
    </Paper>
  )
}

const SortableTile = ({ id, children }: { id: string; children: React.ReactNode }) => {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id
  })

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.92 : 1,
    cursor: 'grab',
    touchAction: 'none'
  }

  return (
    <Box ref={setNodeRef} style={style} {...attributes} {...listeners}>
      {children}
    </Box>
  )
}

const MPH_TO_KMH = 1.609344

export const Telemetry = () => {
  const theme = useTheme()

  const connected = useTelemetryStore((s) => s.connected)
  const connecting = useTelemetryStore((s) => s.connecting)
  const lastMessageAt = useTelemetryStore((s) => s.lastMessageAt)
  const lastError = useTelemetryStore((s) => s.lastError)
  const payload = useTelemetryStore((s) => s.payload)
  const connect = useTelemetryStore((s) => s.connect)
  const disconnect = useTelemetryStore((s) => s.disconnect)

  const tileOrder = useTelemetryStore((s) => s.tileOrder)
  const tileEnabled = useTelemetryStore((s) => s.tileEnabled)
  const setTileEnabled = useTelemetryStore((s) => s.setTileEnabled)
  const setTileOrder = useTelemetryStore((s) => s.setTileOrder)

  const vehicleId = useTelemetryStore((s) => s.vehicleId)
  const supportedVehicles = useTelemetryStore((s) => s.supportedVehicles)
  const setVehicleId = useTelemetryStore((s) => s.setVehicleId)

  const units = useTelemetryStore((s) => s.units)
  const setUnits = useTelemetryStore((s) => s.setUnits)

  const [settingsOpen, setSettingsOpen] = useState(false)

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 6 } }))

  const speedBuf = useRef<number[]>([])
  const rpmBuf = useRef<number[]>([])
  const tempBuf = useRef<number[]>([])
  const consBuf = useRef<number[]>([])
  const rangeBuf = useRef<number[]>([])
  const fuelBuf = useRef<number[]>([])

  useEffect(() => {
    connect()
    return () => disconnect()
  }, [connect, disconnect])

  const wsLive = useMemo(() => {
    if (!connected) return false
    if (!lastMessageAt) return false
    return Date.now() - lastMessageAt < 1200
  }, [connected, lastMessageAt])

  const canOk = useMemo(() => {
    const v = payload?.can_ok
    if (typeof v === 'number') return v === 1
    if (typeof v === 'boolean') return v
    return false
  }, [payload])

  const canErr = useMemo(() => {
    const e = payload?.can_err
    return typeof e === 'string' && e.trim().length ? e : null
  }, [payload])

  const speedKmhRaw = typeof payload?.speedKmh === 'number' ? payload.speedKmh : undefined
  const speedMphRaw = typeof payload?.speedMph === 'number' ? payload.speedMph : undefined

  const speedDisplay = useMemo(() => {
    if (units === 'mph') {
      if (speedMphRaw != null) return speedMphRaw
      if (speedKmhRaw != null) return speedKmhRaw / MPH_TO_KMH
      return undefined
    }
    if (speedKmhRaw != null) return speedKmhRaw
    if (speedMphRaw != null) return speedMphRaw * MPH_TO_KMH
    return undefined
  }, [speedKmhRaw, speedMphRaw, units])

  const speedUnit = units === 'mph' ? 'mph' : 'km/h'

  const rpm = typeof payload?.rpm === 'number' ? payload.rpm : undefined
  const engineTempC = typeof payload?.engineTempC === 'number' ? payload.engineTempC : undefined
  const instantConsumption =
    typeof payload?.instantConsumption === 'number' ? payload.instantConsumption : undefined
  const rangeKm = typeof payload?.rangeKm === 'number' ? payload.rangeKm : undefined
  const fuelLiters = typeof payload?.fuelLiters === 'number' ? payload.fuelLiters : undefined

  const push = (buf: React.MutableRefObject<number[]>, v?: number) => {
    if (v == null || Number.isNaN(v)) return
    const arr = buf.current
    arr.push(v)
    if (arr.length > 50) arr.splice(0, arr.length - 50)
  }

  useEffect(() => {
    push(speedBuf, speedDisplay)
    push(rpmBuf, rpm)
    push(tempBuf, engineTempC)
    push(consBuf, instantConsumption)
    push(rangeBuf, rangeKm)
    push(fuelBuf, fuelLiters)
  }, [engineTempC, fuelLiters, instantConsumption, rangeKm, rpm, speedDisplay])

  const onDragEnd = (event: DragEndEvent) => {
    const { active, over } = event
    if (!over) return
    if (String(active.id) === String(over.id)) return

    const order = tileOrder ?? []
    const oldIndex = order.indexOf(String(active.id))
    const newIndex = order.indexOf(String(over.id))
    if (oldIndex < 0 || newIndex < 0) return

    setTileOrder(arrayMove(order, oldIndex, newIndex))
  }

  const enabledKeys = (tileOrder ?? []).filter((k) => tileEnabled?.[k] !== false)

  const tiles: Record<string, JSX.Element> = {
    speed: (
      <MetricTile
        label="Speed"
        value={speedDisplay}
        unit={speedUnit}
        min={0}
        max={units === 'mph' ? 160 : 260}
        live={wsLive}
        series={speedBuf.current}
        accent="info"
      />
    ),
    rpm: (
      <MetricTile
        label="RPM"
        value={rpm}
        unit="rpm"
        min={0}
        max={8000}
        live={wsLive}
        series={rpmBuf.current}
        accent="primary"
      />
    ),
    temp: (
      <MetricTile
        label="Engine temp"
        value={engineTempC}
        unit="°C"
        min={40}
        max={130}
        decimals={0}
        live={wsLive}
        series={tempBuf.current}
        accent="warning"
      />
    ),
    consumption: (
      <MetricTile
        label="Consumption"
        value={instantConsumption}
        unit="L/100km"
        min={0}
        max={25}
        decimals={1}
        live={wsLive}
        series={consBuf.current}
        accent="primary"
      />
    ),
    range: (
      <MetricTile
        label="Range"
        value={rangeKm}
        unit="km"
        min={0}
        max={1200}
        live={wsLive}
        series={rangeBuf.current}
        accent="success"
      />
    ),
    fuel: (
      <MetricTile
        label="Fuel"
        value={fuelLiters}
        unit="L"
        min={0}
        max={70}
        live={wsLive}
        series={fuelBuf.current}
        accent="success"
      />
    )
  }

  const statusTitle = useMemo(() => {
    if (connecting) return 'Connecting'
    if (!connected) return 'Disconnected'
    if (!wsLive) return 'No data'
    if (canOk) return 'CAN OK'
    return 'CAN error'
  }, [canOk, connected, connecting, wsLive])

  const statusDetail = useMemo(() => {
    if (connecting) return null
    if (!connected) return lastError ?? null
    if (!wsLive) return 'Waiting for telemetry'
    if (canOk) return null
    return canErr ?? 'CAN not available'
  }, [canErr, canOk, connected, connecting, lastError, wsLive])

  return (
    <Box
      sx={{
        p: 1.6,
        height: '100%',
        boxSizing: 'border-box',
        display: 'flex',
        flexDirection: 'column',
        gap: 1.1
      }}
    >
      <Box sx={{ flex: 1, minHeight: 0 }}>
        <DndContext
          sensors={settingsOpen ? ([] as any) : (sensors as any)}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext items={enabledKeys} strategy={rectSortingStrategy}>
            <Box
              sx={{
                display: 'grid',
                gap: 1.15,
                gridTemplateColumns: 'repeat(auto-fit, minmax(190px, 1fr))',
                alignContent: 'start'
              }}
            >
              {enabledKeys.map((key) => (
                <SortableTile key={key} id={key}>
                  {tiles[key] ?? (
                    <Paper
                      elevation={0}
                      sx={{
                        p: 1.5,
                        borderRadius: 3,
                        border: `1px solid ${theme.palette.divider}`,
                        opacity: 0.6
                      }}
                    >
                      <Typography variant="caption" color="text.secondary">
                        Unknown tile: {key}
                      </Typography>
                    </Paper>
                  )}
                </SortableTile>
              ))}
            </Box>
          </SortableContext>
        </DndContext>
      </Box>

      <Box
        sx={{
          height: 3,
          borderRadius: 999,
          bgcolor: canOk ? theme.palette.success.main : theme.palette.error.main,
          boxShadow: canOk ? '0 0 18px rgba(0,255,160,0.22)' : '0 0 18px rgba(255,80,80,0.18)'
        }}
      />

      <Box sx={{ px: 0.2, mt: -0.3 }}>
        <Typography
          variant="caption"
          sx={{ color: theme.palette.text.secondary, fontWeight: 700, letterSpacing: 0.4 }}
        >
          {statusTitle}
        </Typography>
        {statusDetail && (
          <Typography
            variant="caption"
            sx={{ display: 'block', color: theme.palette.text.secondary, opacity: 0.85 }}
          >
            {statusDetail}
          </Typography>
        )}
      </Box>

      <Fab
        size="medium"
        onClick={() => setSettingsOpen(true)}
        sx={{ position: 'fixed', right: 16, bottom: 16, zIndex: 1300 }}
      >
        <SettingsOutlinedIcon />
      </Fab>

      <Drawer
        anchor="right"
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        PaperProps={{ sx: { width: 320, height: '100%', overflow: 'hidden' } }}
      >
        <Box
          sx={{
            height: '100%',
            overflowY: 'auto',
            WebkitOverflowScrolling: 'touch',
            touchAction: 'pan-y',
            p: 2
          }}
        >
          <Typography variant="h6" sx={{ fontWeight: 900, mb: 1 }}>
            Options
          </Typography>

          <Typography variant="overline" color="text.secondary">
            Vehicle
          </Typography>
          <Select
            fullWidth
            size="small"
            value={vehicleId ?? 0}
            onChange={(e) => setVehicleId(Number(e.target.value))}
            sx={{ mb: 2 }}
          >
            {(supportedVehicles ?? []).map((v) => (
              <MenuItem key={v.id} value={v.id}>
                {v.name}
              </MenuItem>
            ))}
          </Select>

          <Typography variant="overline" color="text.secondary">
            Units
          </Typography>
          <Select
            fullWidth
            size="small"
            value={units}
            onChange={(e) => setUnits(String(e.target.value) as 'kmh' | 'mph')}
            sx={{ mb: 2 }}
          >
            <MenuItem value="kmh">km/h</MenuItem>
            <MenuItem value="mph">mph</MenuItem>
          </Select>

          <Divider sx={{ mb: 1.5 }} />

          <Typography variant="overline" color="text.secondary">
            Tiles
          </Typography>
          <Stack spacing={0.25} sx={{ mt: 0.5 }}>
            {(tileOrder ?? []).map((key) => (
              <FormControlLabel
                key={key}
                control={
                  <Checkbox
                    checked={tileEnabled?.[key] !== false}
                    onChange={(e) => setTileEnabled(key, e.target.checked)}
                  />
                }
                label={key}
              />
            ))}
          </Stack>

          <Typography variant="caption" color="text.secondary" sx={{ mt: 2, display: 'block' }}>
            Drag tiles to reorder. Changes are persisted locally.
          </Typography>
        </Box>
      </Drawer>
    </Box>
  )
}
