import { useMemo } from 'react'
import type { CSSProperties } from 'react'
import { Box, Divider, Stack, Typography } from '@mui/material'
import { useCarplayStore, useStatusStore } from '@store/store'

type Row = {
  label: string
  value: string | number | null | undefined
  mono?: boolean
  tooltip?: string
}

type DevListEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
}

type BoxInfoPayload = {
  uuid?: string
  MFD?: string
  boxType?: string
  OemName?: string
  productType?: string
  HiCar?: number
  supportLinkType?: string
  supportFeatures?: string
  hwVersion?: string
  WiFiChannel?: number
  CusCode?: string
  DevList?: DevListEntry[]
}

function normalizeBoxInfo(input: unknown): BoxInfoPayload | null {
  if (!input) return null

  // Sometimes it's already an object
  if (typeof input === 'object') {
    return input as BoxInfoPayload
  }

  // Sometimes a JSON string
  if (typeof input === 'string') {
    const s = input.trim()
    if (!s) return null
    try {
      const parsed = JSON.parse(s)
      if (parsed && typeof parsed === 'object') return parsed as BoxInfoPayload
    } catch {
      // ignore
    }
  }

  return null
}

function fmt(v: unknown): string | null {
  if (v == null) return null
  const s = String(v).trim()
  return s.length ? s : null
}

export function DongleInfo() {
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const isStreaming = useStatusStore((s) => s.isStreaming)

  // USB descriptor
  const vendorId = useCarplayStore((s) => s.vendorId)
  const productId = useCarplayStore((s) => s.productId)
  const usbFwVersion = useCarplayStore((s) => s.usbFwVersion)

  // Dongle Info
  const dongleFwVersion = useCarplayStore((s) => s.dongleFwVersion)
  const boxInfoRaw = useCarplayStore((s) => s.boxInfo)

  // Video stream (negotiated)
  const negotiatedWidth = useCarplayStore((s) => s.negotiatedWidth)
  const negotiatedHeight = useCarplayStore((s) => s.negotiatedHeight)

  // Audio stream
  const audioCodec = useCarplayStore((s) => s.audioCodec)
  const audioSampleRate = useCarplayStore((s) => s.audioSampleRate)
  const audioChannels = useCarplayStore((s) => s.audioChannels)
  const audioBitDepth = useCarplayStore((s) => s.audioBitDepth)

  const boxInfo = useMemo(() => normalizeBoxInfo(boxInfoRaw), [boxInfoRaw])
  const devList = useMemo(
    () => (Array.isArray(boxInfo?.DevList) ? (boxInfo?.DevList ?? []) : []),
    [boxInfo]
  )

  const resolution =
    negotiatedWidth && negotiatedHeight ? `${negotiatedWidth}×${negotiatedHeight}` : '—'

  const audioLine = useMemo(() => {
    const parts: string[] = []
    if (audioCodec) parts.push(String(audioCodec))
    if (audioSampleRate) parts.push(`${audioSampleRate} Hz`)
    if (audioChannels != null) parts.push(`${audioChannels} ch`)
    if (audioBitDepth) parts.push(`${audioBitDepth} bit`)
    return parts.length ? parts.join(' • ') : '—'
  }, [audioCodec, audioSampleRate, audioChannels, audioBitDepth])

  const Mono: CSSProperties = {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
    fontVariantNumeric: 'tabular-nums'
  }

  const rowsTop = useMemo<Row[]>(
    () => [
      { label: 'Dongle', value: isDongleConnected ? 'Connected' : 'Not connected' },
      { label: 'Phone', value: isStreaming ? 'Connected' : 'Not connected' }
    ],
    [isDongleConnected, isStreaming]
  )

  const rowsUsb = useMemo<Row[]>(
    () => [
      {
        label: 'USB Vendor',
        value: vendorId != null ? `0x${vendorId.toString(16)}` : '—',
        mono: true
      },
      {
        label: 'USB Product',
        value: productId != null ? `0x${productId.toString(16)}` : '—',
        mono: true
      },
      { label: 'USB FW', value: usbFwVersion, mono: true }
    ],
    [vendorId, productId, usbFwVersion]
  )

  const rowsDongle = useMemo<Row[]>(
    () => [
      { label: 'Dongle FW', value: dongleFwVersion, mono: true },
      { label: 'UUID', value: fmt(boxInfo?.uuid), mono: true },
      { label: 'MFD', value: fmt(boxInfo?.MFD), mono: true },
      { label: 'ProductType', value: fmt(boxInfo?.productType), mono: true },
      { label: 'HW', value: fmt(boxInfo?.hwVersion), mono: true },
      { label: 'BoxType', value: fmt(boxInfo?.boxType) },
      { label: 'OEM', value: fmt(boxInfo?.OemName) },
      { label: 'WiFi Channel', value: boxInfo?.WiFiChannel ?? null, mono: true },
      { label: 'Links', value: fmt(boxInfo?.supportLinkType) },
      { label: 'Features', value: fmt(boxInfo?.supportFeatures) },
      { label: 'CusCode', value: fmt(boxInfo?.CusCode), mono: true }
    ],
    [dongleFwVersion, boxInfo]
  )

  const rowsStreams = useMemo<Row[]>(
    () => [
      { label: 'Resolution', value: resolution, mono: true },
      { label: 'Audio', value: audioLine, mono: true }
    ],
    [resolution, audioLine]
  )

  const renderRows = (rows: Row[]) => (
    <Stack spacing={0.75}>
      {rows.map((r) => (
        <Stack key={r.label} direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 140 }} color="text.secondary">
            {r.label}:
          </Typography>

          <Typography
            title={r.tooltip ?? (typeof r.value === 'string' ? r.value : '')}
            sx={{
              ...(r.mono ? Mono : null),
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              maxWidth: '52ch'
            }}
          >
            {r.value != null && String(r.value).trim() ? String(r.value) : '—'}
          </Typography>
        </Stack>
      ))}
    </Stack>
  )

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      {renderRows(rowsTop)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        USB Descriptor
      </Typography>
      {renderRows(rowsUsb)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        Dongle Info
      </Typography>
      {renderRows(rowsDongle)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        Streams
      </Typography>
      {renderRows(rowsStreams)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        Paired / Connected Devices
      </Typography>

      {devList.length === 0 ? (
        <Typography variant="body2" color="text.secondary">
          — (no devices reported)
        </Typography>
      ) : (
        <Stack spacing={1}>
          {devList.map((d, i) => {
            const idx = fmt(d.index) ?? String(i + 1)
            const name = fmt(d.name) ?? '—'
            const type = fmt(d.type) ?? '—'
            const id = fmt(d.id) ?? '—'
            const time = fmt(d.time) ?? '—'
            const rfcomm = fmt(d.rfcomm) ?? '—'

            return (
              <Box
                key={`${idx}-${id}-${i}`}
                sx={{
                  border: '1px solid',
                  borderColor: 'divider',
                  borderRadius: 1.5,
                  px: 1.25,
                  py: 0.75
                }}
              >
                <Typography sx={{ ...Mono }}>
                  #{idx} • {name}
                </Typography>

                <Typography variant="body2" color="text.secondary" sx={{ ...Mono }}>
                  {type} • {id}
                </Typography>

                <Typography variant="body2" color="text.secondary" sx={{ ...Mono }}>
                  time={time} • rfcomm={rfcomm}
                </Typography>
              </Box>
            )
          })}
        </Stack>
      )}
    </Box>
  )
}
