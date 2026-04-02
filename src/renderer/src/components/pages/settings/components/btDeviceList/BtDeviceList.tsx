import CloseIcon from '@mui/icons-material/Close'
import LinkIcon from '@mui/icons-material/Link'
import { IconButton, Typography } from '@mui/material'
import { useLiviStore } from '@renderer/store/store'
import type { BoxInfoPayload, DevListEntry } from '@renderer/types'
import { PhoneWorkMode } from '@shared/types'
import { useEffect, useMemo, useRef, useState } from 'react'
import { StackItem } from '../stackItem'

const iconSx = { fontSize: 'clamp(22px, 4.2vh, 34px)' } as const
const btnSx = { padding: 'clamp(4px, 1.2vh, 10px)' } as const

const normalizeMac = (value?: string): string => {
  return value?.trim().toUpperCase() ?? ''
}

const getConnectedMacFromBoxInfo = (boxInfo?: BoxInfoPayload): string => {
  return normalizeMac(boxInfo?.btMacAddr)
}

export const BtDeviceList = () => {
  const devices = useLiviStore((s) => s.bluetoothPairedDevices)
  const boxInfo = useLiviStore((s) => s.boxInfo) as BoxInfoPayload | undefined
  const connectedMac = useMemo(() => getConnectedMacFromBoxInfo(boxInfo), [boxInfo])
  const [pendingConnectMac, setPendingConnectMac] = useState<string>('')
  const deviceMetaCacheRef = useRef<Record<string, { type?: string; index?: number }>>({})

  const remove = useLiviStore((s) => s.removeBluetoothPairedDeviceLocal)
  //const remove = useLiviStore((s) => s.forgetBluetoothPairedDevice)

  const connect = useLiviStore((s) => s.connectBluetoothPairedDevice)
  const saveSettings = useLiviStore((s) => s.saveSettings)

  useEffect(() => {
    const devList = Array.isArray(boxInfo?.DevList) ? boxInfo.DevList : []

    for (const entry of devList) {
      const mac = normalizeMac((entry as DevListEntry).id)
      if (!mac) continue

      deviceMetaCacheRef.current[mac] = {
        type: (entry as DevListEntry).type,
        index: Number((entry as DevListEntry).index ?? 999)
      }
    }

    if (connectedMac) {
      setPendingConnectMac('')
    }
  }, [boxInfo, connectedMac])

  const sortedList = useMemo(() => {
    if (!Array.isArray(devices)) return []

    // Map each device to include type info
    const enriched = devices.map((d) => {
      const mac = normalizeMac(d.mac)
      const devEntry = boxInfo?.DevList?.find((b: DevListEntry) => normalizeMac(b.id) === mac)
      const cached = deviceMetaCacheRef.current[mac]

      const type = devEntry?.type ?? cached?.type ?? 'Unknown'
      const index = Number(devEntry?.index ?? cached?.index ?? 999)
      const targetPhoneWorkMode =
        type === 'AndroidAuto' ? PhoneWorkMode.Android : PhoneWorkMode.CarPlay

      return { ...d, mac, type, index, targetPhoneWorkMode }
    })

    return enriched.sort((a, b) => a.index - b.index)
  }, [devices, boxInfo])

  return (
    <>
      {sortedList.map((d) => {
        const name = d.name?.trim()
        const label = name && name.length > 0 ? name : 'Unknown device'
        const isConnected = d.mac === connectedMac
        const isConnecting = d.mac === pendingConnectMac
        const isSwitching = pendingConnectMac.length > 0
        const typeLabel =
          d.type === 'AndroidAuto' ? 'Android Auto' : d.type === 'CarPlay' ? 'CarPlay' : ''

        return (
          <StackItem key={d.mac}>
            <Typography
              component="p"
              sx={{
                minWidth: 0,
                display: 'flex',
                alignItems: 'center',
                overflow: 'hidden',
                flex: 1
              }}
            >
              <span
                style={{
                  minWidth: 0,
                  flex: 1,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                  color: isConnected ? 'var(--ui-highlight)' : 'inherit'
                }}
              >
                {label}
              </span>

              <span
                style={{
                  flexShrink: 0,
                  width: 'clamp(120px, 16vw, 170px)',
                  textAlign: 'right',
                  opacity: typeLabel ? 0.6 : 0,
                  fontSize: '0.9em',
                  whiteSpace: 'nowrap'
                }}
              >
                {typeLabel || ' '}
              </span>
            </Typography>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <IconButton
                sx={btnSx}
                disabled={isConnected || isSwitching}
                onClick={async () => {
                  setPendingConnectMac(d.mac)

                  await saveSettings({
                    lastPhoneWorkMode: d.targetPhoneWorkMode
                  })

                  const ok = await connect(d.mac)
                  if (!ok) {
                    setPendingConnectMac('')
                    return
                  }

                  try {
                    await window.projection.usb.forceReset()
                  } catch (e) {
                    console.warn('[BtDeviceList] usb.forceReset() failed during device switch', e)
                    setPendingConnectMac('')
                  }
                }}
              >
                <LinkIcon
                  sx={{
                    ...iconSx,
                    opacity: isConnected || isSwitching ? 0.3 : 1,
                    color: isConnected || isSwitching ? 'action.disabled' : 'inherit',
                    transition: isConnecting ? 'none !important' : undefined,
                    ...(isConnecting
                      ? {
                          opacity: 'var(--ui-breathe-opacity, 1)',
                          color: 'var(--ui-highlight)'
                        }
                      : {})
                  }}
                />
              </IconButton>

              <IconButton sx={btnSx} disabled={isSwitching} onClick={() => remove(d.mac)}>
                <CloseIcon
                  sx={{
                    ...iconSx,
                    opacity: isSwitching ? 0.3 : 1,
                    color: isSwitching ? 'action.disabled' : 'inherit'
                  }}
                />
              </IconButton>
            </div>
          </StackItem>
        )
      })}
    </>
  )
}
