import AndroidIcon from '@mui/icons-material/Android'
import BoltIcon from '@mui/icons-material/Bolt'
import CableOutlinedIcon from '@mui/icons-material/CableOutlined'
import CloseIcon from '@mui/icons-material/Close'
import DeviceHubIcon from '@mui/icons-material/DeviceHub'
import DirectionsCarIcon from '@mui/icons-material/DirectionsCar'
import PhoneIphoneIcon from '@mui/icons-material/PhoneIphone'
import WifiOutlinedIcon from '@mui/icons-material/WifiOutlined'
import { IconButton, Typography, useTheme } from '@mui/material'
import type { DeviceView } from '@shared/types'
import { StackItem } from '../stackItem'

const ProtocolIcon = ({ p, size }: { p?: DeviceView['protocol']; size: number | string }) =>
  p === 'carplay' ? (
    <PhoneIphoneIcon sx={{ fontSize: size }} />
  ) : p === 'androidauto' ? (
    <AndroidIcon sx={{ fontSize: size }} />
  ) : (
    <DirectionsCarIcon sx={{ fontSize: size }} />
  )

const SourceBadge = ({ d, size }: { d: DeviceView; size: number }) => {
  if (d.lastTransport === 'usb') return <CableOutlinedIcon sx={{ fontSize: size }} />
  if (d.lastTransport === 'wifi') return <WifiOutlinedIcon sx={{ fontSize: size }} />
  return null
}

const batteryColor = (pct: number): string =>
  pct < 10 ? '#ff3b30' : pct < 20 ? '#ffcc00' : '#34c759'

const BatteryIcon = ({ level, charging }: { level: number; charging?: boolean }) => {
  const theme = useTheme()
  const pct = Math.max(0, Math.min(100, Math.round(level)))
  const fillW = Math.max(3, (42 * pct) / 100)
  const outline = theme.palette.text.secondary
  return (
    <span
      role="img"
      aria-label={`${pct}%${charging ? ' charging' : ''}`}
      style={{
        position: 'relative',
        display: 'inline-flex',
        width: 52,
        height: 24,
        alignItems: 'center'
      }}
    >
      <svg width={52} height={24} viewBox="0 0 52 24" style={{ position: 'absolute', inset: 0 }}>
        <rect
          x={1}
          y={3.5}
          width={45}
          height={17}
          rx={4}
          fill="none"
          stroke={outline}
          strokeWidth={1.8}
        />
        <rect x={47.5} y={8.5} width={3} height={7} rx={1.5} fill={outline} />
        <rect
          x={3}
          y={5.5}
          width={fillW}
          height={13}
          rx={2}
          fill={batteryColor(pct)}
          opacity={0.9}
        />
        {/* Digits as svg text: geometric centering that scales with UI zoom exactly like the
            battery (an HTML text box drifts with the zoomed font metrics). Right-anchored
            tabular digits, the ones digit never moves. */}
        <text
          x={39}
          y={12}
          textAnchor="end"
          dominantBaseline="central"
          fontSize={10.5}
          fontWeight={700}
          fill={theme.palette.text.primary}
          stroke={theme.palette.background.paper}
          strokeWidth={2}
          paintOrder="stroke"
          style={{ fontVariantNumeric: 'tabular-nums' }}
        >
          {pct}
        </text>
      </svg>
      {charging ? (
        <BoltIcon
          sx={{
            // Anchored left of the fixed digit block: charging never shifts the SoC.
            position: 'absolute',
            left: 2,
            top: '50%',
            transform: 'translateY(-50%)',
            fontSize: 12,
            color: theme.palette.text.primary
          }}
        />
      ) : null}
    </span>
  )
}

const SIGNAL_HEIGHTS = [4, 6.5, 9, 11.5, 14]

const SignalBars = ({ level }: { level: number }) => {
  const theme = useTheme()
  const n = Math.max(0, Math.min(SIGNAL_HEIGHTS.length, Math.round(level)))
  return (
    <span style={{ display: 'inline-flex', alignItems: 'flex-end', gap: 2, height: 14 }}>
      {SIGNAL_HEIGHTS.map((h, i) => {
        const on = i < n
        return (
          <span
            key={h}
            style={{
              width: 3,
              height: h,
              borderRadius: 1,
              background: on ? theme.palette.text.primary : theme.palette.text.disabled,
              opacity: on ? 1 : 0.35
            }}
          />
        )
      })}
    </span>
  )
}

/** Equal-width status marker so rows never shift: filled text-color = available,
 *  filled accent = active, dimmed outline = offline. */
const StatusDot = ({ status }: { status: DeviceView['status'] }) => {
  const theme = useTheme()
  const fill =
    status === 'active'
      ? theme.palette.secondary.main
      : status === 'available'
        ? theme.palette.text.primary
        : 'transparent'
  return (
    <span
      role="status"
      aria-label={status}
      style={{
        display: 'inline-block',
        width: 12,
        height: 12,
        borderRadius: 6,
        boxSizing: 'border-box',
        background: fill,
        border: status === 'offline' ? `2px solid ${theme.palette.text.disabled}` : 'none'
      }}
    />
  )
}

type Props = {
  device: DeviceView
  onSelect?: () => void
  onForget: () => void
}

/** One paired device as a standard settings row: label side carries the protocol
 *  marker and name, the value side keeps fixed-width slots so missing info never
 *  shifts the layout. */
export const SettingsDeviceRow = ({ device: d, onSelect, onForget }: Props) => {
  const theme = useTheme()
  const active = d.status === 'active'
  const offline = d.status === 'offline'
  const accent = active ? theme.palette.secondary.main : theme.palette.text.secondary

  return (
    <StackItem onClick={onSelect} ownIcon dimmed={offline}>
      <span
        style={{
          flex: 'none',
          display: 'inline-flex',
          color: accent,
          marginLeft: 'var(--livi-row-pad, 12px)'
        }}
      >
        <ProtocolIcon p={d.protocol} size="var(--livi-row-icon, 24px)" />
      </span>
      <Typography
        sx={{ minWidth: 0, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}
      >
        {d.name || d.model || d.id}
      </Typography>

      <span
        style={{
          flex: 'none',
          display: 'inline-flex',
          alignItems: 'center',
          gap: 'clamp(10px, 1.9svh, 16px)',
          color: theme.palette.text.secondary
        }}
      >
        <span style={{ width: 26, display: 'inline-flex', justifyContent: 'center' }}>
          <SourceBadge d={d} size={18} />
        </span>
        <span style={{ width: 52, display: 'inline-flex', justifyContent: 'center' }}>
          {typeof d.batteryLevel === 'number' ? (
            <BatteryIcon level={d.batteryLevel} charging={d.batteryCharging} />
          ) : null}
        </span>
        <span style={{ width: 26, display: 'inline-flex', justifyContent: 'center' }}>
          {typeof d.signalStrength === 'number' ? <SignalBars level={d.signalStrength} /> : null}
        </span>
        <span style={{ width: 20, display: 'inline-flex', justifyContent: 'center' }}>
          <StatusDot status={d.status} />
        </span>
        <IconButton
          aria-label="Delete device"
          className="nav-focus-primary"
          onClick={(e) => {
            e.stopPropagation()
            onForget()
          }}
          onKeyDown={(e) => e.stopPropagation()}
          sx={{
            flex: 'none',
            width: 'clamp(30px, 4.8svh, 40px)',
            height: 'clamp(30px, 4.8svh, 40px)',
            p: 0,
            color: theme.palette.text.secondary
          }}
        >
          <CloseIcon sx={{ fontSize: 'clamp(18px, 3.2svh, 22px)' }} />
        </IconButton>
      </span>
    </StackItem>
  )
}
