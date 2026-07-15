import AndroidIcon from '@mui/icons-material/Android'
import BoltIcon from '@mui/icons-material/Bolt'
import CableOutlinedIcon from '@mui/icons-material/CableOutlined'
import CloseIcon from '@mui/icons-material/Close'
import DeviceHubIcon from '@mui/icons-material/DeviceHub'
import DirectionsCarIcon from '@mui/icons-material/DirectionsCar'
import PhoneIphoneIcon from '@mui/icons-material/PhoneIphone'
import WifiOutlinedIcon from '@mui/icons-material/WifiOutlined'
import { useTheme } from '@mui/material'
import { useLiviStore } from '@renderer/store/store'
import { useNavigate } from 'react-router'
import { type DeviceView, forgetDevice, selectDevice, useDevices } from './useDevices'

const protocolLabel = (p?: DeviceView['protocol']): string =>
  p === 'carplay' ? 'CarPlay' : p === 'androidauto' ? 'Android Auto' : 'Device'

const ProtocolIcon = ({ p, size }: { p?: DeviceView['protocol']; size: number }) =>
  p === 'carplay' ? (
    <PhoneIphoneIcon sx={{ fontSize: size }} />
  ) : p === 'androidauto' ? (
    <AndroidIcon sx={{ fontSize: size }} />
  ) : (
    <DirectionsCarIcon sx={{ fontSize: size }} />
  )

const SourceBadge = ({ d, size }: { d: DeviceView; size: number }) => {
  if (d.source === 'dongle') return <DeviceHubIcon sx={{ fontSize: size }} />
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
      title={`${pct}%${charging ? ' charging' : ''}`}
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
      </svg>
      <span
        style={{
          position: 'relative',
          width: 46,
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 1,
          fontSize: 10.5,
          fontWeight: 700,
          lineHeight: 1,
          color: theme.palette.text.primary,
          textShadow: `0 0 2px ${theme.palette.background.paper}, 0 0 2px ${theme.palette.background.paper}`
        }}
      >
        {charging ? <BoltIcon sx={{ fontSize: 12, color: '#34c759' }} /> : null}
        {pct}
      </span>
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

export const Devices = () => {
  const devices = useDevices()
  const theme = useTheme()
  const navigate = useNavigate()
  const forgetDongle = useLiviStore((s) => s.forgetBluetoothPairedDevice)

  const onPick = (d: DeviceView) => {
    if (d.status === 'offline') return
    selectDevice(d.id)
    navigate('/')
  }

  return (
    <div
      style={{
        width: '100%',
        minHeight: '100%',
        flexGrow: 1,
        display: 'flex',
        flexWrap: 'wrap',
        justifyContent: 'center',
        alignItems: 'center',
        alignContent: 'center',
        gap: 20,
        padding: 20,
        boxSizing: 'border-box'
      }}
    >
      {devices.length === 0 ? (
        <span style={{ color: theme.palette.text.secondary, fontSize: 16 }}>No paired devices</span>
      ) : null}

      {devices.map((d) => {
        const active = d.status === 'active'
        const offline = d.status === 'offline'
        const accent = active
          ? theme.palette.secondary.main
          : offline
            ? theme.palette.text.disabled
            : theme.palette.text.primary
        return (
          <div
            key={d.id}
            style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 }}
          >
            <button
              type="button"
              onClick={() => onPick(d)}
              disabled={offline}
              style={{
                position: 'relative',
                width: 'clamp(150px, 24vw, 210px)',
                height: 'clamp(210px, 42vh, 300px)',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: 12,
                padding: 20,
                borderRadius: 18,
                border: `3px solid ${accent}`,
                background: theme.palette.background.paper,
                color: theme.palette.text.primary,
                opacity: offline ? 0.4 : 1,
                cursor: offline ? 'default' : 'pointer'
              }}
            >
              {typeof d.session === 'number' ? (
                <span
                  style={{
                    position: 'absolute',
                    top: 10,
                    right: 12,
                    minWidth: 24,
                    height: 24,
                    padding: '0 7px',
                    borderRadius: 12,
                    boxSizing: 'border-box',
                    border: `2px solid ${accent}`,
                    color: accent,
                    fontSize: 13,
                    fontWeight: 700,
                    lineHeight: 1,
                    display: 'inline-flex',
                    alignItems: 'center',
                    justifyContent: 'center'
                  }}
                >
                  {d.session}
                </span>
              ) : null}
              <div style={{ color: accent, marginTop: 8 }}>
                <ProtocolIcon p={d.protocol} size={64} />
              </div>

              <div style={{ textAlign: 'center' }}>
                <div style={{ fontSize: 20, fontWeight: 700, lineHeight: 1.2 }}>
                  {d.name || d.model || d.id}
                </div>
                <div
                  style={{
                    fontSize: 13,
                    color: theme.palette.text.secondary,
                    marginTop: 4,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 4
                  }}
                >
                  {protocolLabel(d.protocol)}
                  <SourceBadge d={d} size={15} />
                </div>
              </div>

              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  gap: 4
                }}
              >
                <span
                  style={{
                    minHeight: 24,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 12,
                    fontSize: 12,
                    color: theme.palette.text.secondary
                  }}
                >
                  {typeof d.batteryLevel === 'number' ? (
                    <BatteryIcon level={d.batteryLevel} charging={d.batteryCharging} />
                  ) : null}
                  {typeof d.signalStrength === 'number' ? (
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 5 }}>
                      <SignalBars level={d.signalStrength} />
                      {d.carrierName ? (
                        <span style={{ fontSize: 11, fontWeight: 600 }}>{d.carrierName}</span>
                      ) : null}
                    </span>
                  ) : null}
                </span>
                <span style={{ fontSize: 13, fontWeight: 600, color: accent }}>
                  {active ? 'Active' : offline ? 'Not available' : 'Available'}
                </span>
              </div>
            </button>
            <button
              type="button"
              onClick={() => (d.source === 'dongle' ? forgetDongle(d.id) : forgetDevice(d.id))}
              aria-label="Delete device"
              style={{
                width: 34,
                height: 34,
                borderRadius: 17,
                border: `2px solid ${theme.palette.text.disabled}`,
                background: 'transparent',
                color: theme.palette.text.secondary,
                cursor: 'pointer',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center'
              }}
            >
              <CloseIcon sx={{ fontSize: 20 }} />
            </button>
          </div>
        )
      })}
    </div>
  )
}
