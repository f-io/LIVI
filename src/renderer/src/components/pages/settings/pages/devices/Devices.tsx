import { Typography } from '@mui/material'
import { useLiviStore } from '@renderer/store/store'
import { SettingsDeviceRow } from '@settings/components'
import { useNavigate } from 'react-router'
import { type DeviceView, forgetDevice, selectDevice, useDevices } from './useDevices'

export const Devices = () => {
  const devices = useDevices()
  const navigate = useNavigate()

  const onPick = async (d: DeviceView) => {
    const r = await selectDevice(d.id)
    if (r.ok) navigate('/')
  }

  return (
    <>
      {devices.length === 0 ? (
        <Typography sx={{ padding: 'clamp(10px, 1.9svh, 16px)', color: 'text.secondary' }}>
          No paired devices
        </Typography>
      ) : null}

      {devices.map((d) => {
        const selectable = typeof d.session === 'number' && d.status !== 'offline'
        return (
          <SettingsDeviceRow
            key={d.id}
            device={d}
            onSelect={selectable ? () => onPick(d) : undefined}
            onForget={() => forgetDevice(d.id)}
          />
        )
      })}
    </>
  )
}
