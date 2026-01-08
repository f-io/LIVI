import { useEffect, useMemo, useState } from 'react'
import { Box, Stack, Typography } from '@mui/material'
import { useCarplayStore, useStatusStore } from '@store/store'

function abbreviateManufacturer(name?: string, max = 24): string | undefined {
  if (!name) return name
  let s = name.trim()

  const repl: Array<[RegExp, string]> = [
    [/\b(Communications?|Kommunikation(en)?)\b/gi, 'Comm.'],
    [/\b(Technology|Technologies)\b/gi, 'Tech.'],
    [/\b(Electronics)\b/gi, 'Elec.'],
    [/\b(International)\b/gi, 'Intl.'],
    [/\b(Manufacturing)\b/gi, 'Mfg.'],
    [/\b(Systems)\b/gi, 'Sys.'],
    [/\b(Corporation)\b/gi, 'Corp.'],
    [/\b(Company)\b/gi, 'Co.'],
    [/\b(Limited)\b/gi, 'Ltd.'],
    [/\b(Incorporated)\b/gi, 'Inc.'],
    [/\b(Industries)\b/gi, 'Ind.'],
    [/\b(Laboratories)\b/gi, 'Labs'],
    [/\b(Semiconductors?)\b/gi, 'Semi']
  ]
  for (const [re, to] of repl) s = s.replace(re, to)
  if (s.length <= max) return s

  const parts = s.split(/\s+/).filter(Boolean)
  if (parts.length > 1) {
    const first = parts[0]
    const rest = parts.slice(1).map((p) => {
      const core = p.replace(/[.,]/g, '')
      const cut = Math.min(4, Math.max(3, Math.ceil(core.length * 0.4)))
      return core.slice(0, cut) + '.'
    })
    s = [first, ...rest].join(' ')
    if (s.length <= max) return s

    const initials = parts.slice(1).map((p) => (p[0] ? p[0].toUpperCase() + '.' : ''))
    s = [first, ...initials].join(' ')
    if (s.length <= max) return s
  }

  return s.slice(0, Math.max(0, max - 1)) + '…'
}

type Row = {
  label: string
  value: string | number | null | undefined
  mono?: boolean
  tooltip?: string
}

export function DongleInfo() {
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const isStreaming = useStatusStore((s) => s.isStreaming)

  // Dongle info
  const serial = useCarplayStore((s) => s.serial)
  const manufacturer = useCarplayStore((s) => s.manufacturer)
  const product = useCarplayStore((s) => s.product)
  const fwVersion = useCarplayStore((s) => s.fwVersion)

  // Video stream (negotiated)
  const negotiatedWidth = useCarplayStore((s) => s.negotiatedWidth)
  const negotiatedHeight = useCarplayStore((s) => s.negotiatedHeight)

  // Audio stream
  const audioCodec = useCarplayStore((s) => s.audioCodec)
  const audioSampleRate = useCarplayStore((s) => s.audioSampleRate)
  const audioChannels = useCarplayStore((s) => s.audioChannels)
  const audioBitDepth = useCarplayStore((s) => s.audioBitDepth)

  const displayManufacturer = useMemo(
    () => abbreviateManufacturer(manufacturer ?? undefined, 28),
    [manufacturer]
  )

  const [message, setMessage] = useState<string>('')

  useEffect(() => {
    let cancelled = false

    const run = async () => {
      try {
        setMessage('')

        if (!isDongleConnected) {
          useCarplayStore.getState().resetInfo()
          return
        }

        const info = await window.carplay?.usb?.getDeviceInfo?.()
        if (cancelled) return

        const hasAny =
          !!info?.device ||
          !!info?.serialNumber ||
          !!info?.manufacturerName ||
          !!info?.productName ||
          !!info?.fwVersion

        if (!hasAny) {
          useCarplayStore.getState().resetInfo()
          setMessage('Dongle info not available.')
          return
        }

        useCarplayStore.setState({
          serial: info?.serialNumber,
          manufacturer: info?.manufacturerName,
          product: info?.productName,
          fwVersion: info?.fwVersion
        })
      } catch (err) {
        if (cancelled) return
        console.warn('[DongleInfo] getDeviceInfo failed', err)
        setMessage('Failed to read dongle info.')
      }
    }

    run()
    return () => {
      cancelled = true
    }
  }, [isDongleConnected])

  const resolution =
    negotiatedWidth && negotiatedHeight ? `${negotiatedWidth}×${negotiatedHeight}` : '—'

  const audioLine = (() => {
    const parts: string[] = []
    if (audioCodec) parts.push(String(audioCodec))
    if (audioSampleRate) parts.push(`${audioSampleRate} Hz`)
    if (audioChannels != null) parts.push(`${audioChannels} ch`)
    if (audioBitDepth) parts.push(`${audioBitDepth} bit`)
    return parts.length ? parts.join(' • ') : '—'
  })()

  const rows = useMemo<Row[]>(
    () => [
      { label: 'Dongle', value: isDongleConnected ? 'Connected' : 'Not connected' },
      { label: 'Phone', value: isStreaming ? 'Connected' : 'Not connected' },

      { label: 'Serial', value: serial, mono: true },
      { label: 'Manufacturer', value: displayManufacturer, tooltip: manufacturer || undefined },
      { label: 'Product', value: product },
      { label: 'Firmware', value: fwVersion, mono: true },

      { label: 'Resolution', value: resolution, mono: true },
      { label: 'Audio', value: audioLine, mono: true }
    ],
    [
      isDongleConnected,
      isStreaming,
      serial,
      displayManufacturer,
      manufacturer,
      product,
      fwVersion,
      resolution,
      audioLine
    ]
  )

  const Mono: React.CSSProperties = {
    fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace',
    fontVariantNumeric: 'tabular-nums'
  }

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Stack spacing={0.75}>
        {rows.map((r) => (
          <Stack key={r.label} direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
            <Typography sx={{ minWidth: 110 }} color="text.secondary">
              {r.label}:
            </Typography>

            <Typography
              title={r.tooltip ?? (typeof r.value === 'string' ? r.value : '')}
              sx={{
                ...(r.mono ? Mono : null),
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                maxWidth: '42ch'
              }}
            >
              {r.value != null && String(r.value).trim() ? String(r.value) : '—'}
            </Typography>
          </Stack>
        ))}
      </Stack>

      {message && (
        <Typography variant="body2" color="text.secondary">
          {message}
        </Typography>
      )}
    </Box>
  )
}
