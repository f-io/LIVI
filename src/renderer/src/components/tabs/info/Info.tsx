import { useEffect, useMemo, useState } from 'react'
import {
  Typography,
  Box,
  Stack,
  Button,
  Dialog,
  DialogTitle,
  DialogContent,
  LinearProgress,
  Chip,
  Tooltip
} from '@mui/material'
import { useTheme } from '@mui/material/styles'
import { useCarplayStore, useStatusStore } from '@store/store'
import { FFTSpectrum } from '../fft'

// Abbreviate names
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

export const Info = () => {
  const theme = useTheme()

  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)

  const negotiatedWidth = useCarplayStore((s) => s.negotiatedWidth)
  const negotiatedHeight = useCarplayStore((s) => s.negotiatedHeight)
  const serial = useCarplayStore((s) => s.serial)
  const manufacturer = useCarplayStore((s) => s.manufacturer)
  const product = useCarplayStore((s) => s.product)
  const fwVersion = useCarplayStore((s) => s.fwVersion)

  const audioCodec = useCarplayStore((s) => s.audioCodec)
  const audioSampleRate = useCarplayStore((s) => s.audioSampleRate)
  const audioChannels = useCarplayStore((s) => s.audioChannels)
  const audioBitDepth = useCarplayStore((s) => s.audioBitDepth)

  const isStreaming = useStatusStore((s) => s.isStreaming)
  const pcmData = useCarplayStore((s) => s.audioPcmData) ?? new Float32Array(0)

  const [installedVersion, setInstalledVersion] = useState<string>('—')
  const [latestVersion, setLatestVersion] = useState<string>('—')
  const [latestUrl, setLatestUrl] = useState<string | undefined>(undefined)

  const [upDialogOpen, setUpDialogOpen] = useState(false)
  const [phase, setPhase] = useState<string>('start')
  const [percent, setPercent] = useState<number | null>(null)
  const [received, setReceived] = useState<number>(0)
  const [total, setTotal] = useState<number>(0)
  const [error, setError] = useState<string>('')

  // Grid const
  const HW_MIN_COL = 'calc(112px + 20ch)'

  // UI helpers
  const SectionHeader: React.FC<{ children: React.ReactNode }> = ({ children }) => (
    <Typography
      variant="overline"
      sx={{
        display: 'block',
        letterSpacing: 0.8,
        color: theme.palette.text.secondary,
        mb: 1.25,
        '&::after': {
          content: '""',
          display: 'block',
          height: 2,
          width: 28,
          mt: 0.5,
          backgroundColor: theme.palette.primary.main,
          opacity: 0.6,
          borderRadius: 1
        }
      }}
    >
      {children}
    </Typography>
  )

  type RowOpts = { mono?: boolean; maxCh?: number; color?: string; tooltip?: string }
  type RowValue = React.ReactNode | string | number | null | undefined
  const Row = (label: string, value: RowValue, opts: RowOpts = {}) => {
    const color =
      opts.color ??
      (value != null && value !== '—' ? theme.palette.primary.main : theme.palette.text.primary)

    const isString = typeof value === 'string' || typeof value === 'number'
    const tooltipTitle = opts.tooltip ?? (isString ? String(value) : '')

    return (
      <Stack direction="row" spacing={1} alignItems="baseline">
        <Typography sx={{ minWidth: 112, color: theme.palette.text.secondary }}>
          {label}:
        </Typography>
        <Tooltip title={tooltipTitle} disableInteractive>
          {isString ? (
            <Typography
              component="span"
              sx={{
                color,
                fontVariantNumeric: 'tabular-nums',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                maxWidth: `${opts.maxCh ?? 26}ch`,
                fontFamily: opts.mono
                  ? 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace'
                  : undefined
              }}
            >
              {value ?? '—'}
            </Typography>
          ) : (
            <Box
              component="div"
              sx={{
                color,
                fontVariantNumeric: 'tabular-nums',
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                maxWidth: `${opts.maxCh ?? 26}ch`,
                fontFamily: opts.mono
                  ? 'ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace'
                  : undefined,
                display: 'inline-flex',
                alignItems: 'center'
              }}
            >
              {value ?? '—'}
            </Box>
          )}
        </Tooltip>
      </Stack>
    )
  }

  const parseSemver = (v?: string): number[] | null => {
    if (!v) return null
    const m = v.trim().match(/^(\d+)\.(\d+)\.(\d+)$/)
    if (!m) return null
    return [parseInt(m[1], 10), parseInt(m[2], 10), parseInt(m[3], 10)]
  }
  const cmpSemver = (a: number[], b: number[]) => {
    for (let i = 0; i < 3; i++) {
      if ((a[i] || 0) < (b[i] || 0)) return -1
      if ((a[i] || 0) > (b[i] || 0)) return 1
    }
    return 0
  }

  const installedSem = useMemo(() => parseSemver(installedVersion), [installedVersion])
  const latestSem = useMemo(() => parseSemver(latestVersion), [latestVersion])
  const hasLatest = !!latestUrl && !!latestSem
  const cmp = useMemo(
    () => (installedSem && latestSem ? cmpSemver(installedSem, latestSem) : null),
    [installedSem, latestSem]
  )

  const actionLabel = !hasLatest
    ? 'CHECK'
    : cmp! < 0
      ? 'UPDATE'
      : cmp! > 0
        ? 'DOWNGRADE'
        : 'UP TO DATE'
  const actionEnabled = !hasLatest ? true : cmp! !== null && cmp! !== 0

  const recheckLatest = async () => {
    try {
      const r = await window.app?.getLatestRelease?.()
      if (r?.version) setLatestVersion(r.version)
      else setLatestVersion('—')
      setLatestUrl(r?.url)
    } catch {
      setLatestVersion('—')
      setLatestUrl(undefined)
    }
  }

  useEffect(() => {
    window.app?.getVersion?.().then((v) => v && setInstalledVersion(v))
    recheckLatest()
  }, [])

  useEffect(() => {
    if (isDongleConnected) {
      window.carplay.usb.getDeviceInfo().then((info) => {
        if (info.device) {
          useCarplayStore.setState({
            serial: info.serialNumber,
            manufacturer: info.manufacturerName,
            product: info.productName,
            fwVersion: info.fwVersion
          })
        }
      })
    } else {
      useCarplayStore.getState().resetInfo()
    }
  }, [isDongleConnected])

  useEffect(() => {
    const off1 = window.app?.onUpdateEvent?.((e: UpdateEvent) => {
      if (e.phase === 'error') setError(e.message ?? 'Update failed')
      setPhase(e.phase)
    })

    const off2 = window.app?.onUpdateProgress?.((p: UpdateProgress) => {
      setPhase('download')
      setPercent(typeof p.percent === 'number' ? Math.max(0, Math.min(1, p.percent)) : null)
      setReceived(p.received ?? 0)
      setTotal(p.total ?? 0)
    })

    return () => {
      off1?.()
      off2?.()
    }
  }, [])

  const triggerUpdate = () => {
    setError('')
    setPercent(null)
    setReceived(0)
    setTotal(0)
    setPhase('start')
    setUpDialogOpen(true)
    window.app?.performUpdate?.(latestUrl)
  }

  const onPrimaryAction = () => {
    if (!hasLatest) {
      recheckLatest()
      return
    }
    if (cmp !== 0) triggerUpdate()
  }

  const pct = percent != null ? Math.round(percent * 100) : null
  const human = (n: number) =>
    n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`

  const phaseText =
    phase === 'download'
      ? 'Downloading'
      : phase === 'installing'
        ? 'Installing'
        : phase === 'mounting'
          ? 'Mounting image'
          : phase === 'copying'
            ? 'Copying'
            : phase === 'unmounting'
              ? 'Finalizing'
              : phase === 'relaunching'
                ? 'Relaunching'
                : phase === 'start'
                  ? 'Starting…'
                  : phase === 'error'
                    ? 'Error'
                    : 'Working…'

  const isUpToDate = hasLatest && cmp === 0

  const displayManufacturer = useMemo(
    () => abbreviateManufacturer(manufacturer ?? undefined, 24),
    [manufacturer]
  )

  return (
    <Box
      id="info-root"
      className={theme.palette.mode === 'dark' ? 'App-header-dark' : 'App-header-light'}
      p={2}
      display="flex"
      flexDirection="column"
      height="100vh"
    >
      <Box
        sx={{
          overflowY: 'auto',
          overflowX: 'hidden',
          flexGrow: 1,
          px: 1.5,
          py: 1.5,
          display: 'flex',
          flexDirection: 'column',
          gap: 2
        }}
      >
        {/* Section A: 3-column grid */}
        <Box
          sx={{
            display: 'grid',
            gridTemplateColumns: {
              xs: '1fr',
              sm: `minmax(${HW_MIN_COL}, 1.4fr) minmax(0,1fr) minmax(0,1fr)`
            },
            columnGap: 2,
            rowGap: 2,
            px: 1
          }}
        >
          {/* Col 1: Hardware */}
          <Box sx={{ minWidth: 0 }}>
            <SectionHeader>HARDWARE INFO</SectionHeader>
            <Stack spacing={0.5} sx={{ pl: 1.5 }}>
              {Row('Serial', serial, { mono: true, maxCh: 28 })}
              {Row('Manufacturer', displayManufacturer, {
                maxCh: 28,
                tooltip: manufacturer || undefined
              })}
              {Row('Product', product, { maxCh: 28 })}
              {Row('Firmware', fwVersion, { mono: true, maxCh: 18 })}
            </Stack>
          </Box>

          {/* Col 2: Phone + Video */}
          <Box
            sx={{
              minWidth: 0,
              pl: { md: 2 },
              borderLeft: { md: `1px solid ${theme.palette.divider}` }
            }}
          >
            <SectionHeader>PHONE</SectionHeader>
            <Stack spacing={0.5} sx={{ pl: 1.5, mb: 2 }}>
              {Row(
                'Connected',
                <Chip
                  label={isStreaming ? 'Yes' : 'No'}
                  size="small"
                  variant="outlined"
                  color={isStreaming ? 'success' : 'default'}
                  sx={{ height: 20 }}
                />,
                { maxCh: 20 }
              )}
            </Stack>

            <SectionHeader>VIDEO INFO</SectionHeader>
            <Stack spacing={0.5} sx={{ pl: 1.5 }}>
              {Row(
                'Resolution',
                negotiatedWidth && negotiatedHeight
                  ? `${negotiatedWidth}×${negotiatedHeight}`
                  : '—',
                { mono: true, maxCh: 18 }
              )}
            </Stack>
          </Box>

          {/* Col 3: Software */}
          <Box
            sx={{
              minWidth: 0,
              pl: { md: 2 },
              borderLeft: { md: `1px solid ${theme.palette.divider}` }
            }}
          >
            <SectionHeader>SOFTWARE</SectionHeader>
            <Stack spacing={1} sx={{ pl: 1.5 }}>
              {Row('Installed', installedVersion, { mono: true, maxCh: 10 })}
              {Row('Available', latestVersion, { mono: true, maxCh: 10 })}
              {isUpToDate ? (
                <Chip label="UP TO DATE" size="small" variant="outlined" sx={{ opacity: 0.75 }} />
              ) : (
                <Button
                  size="small"
                  variant="outlined"
                  disabled={!actionEnabled}
                  onClick={onPrimaryAction}
                  sx={{ alignSelf: 'flex-start' }}
                >
                  {actionLabel}
                </Button>
              )}
            </Stack>
          </Box>
        </Box>

        {/* Section B: Audio + FFT */}
        <Box sx={{ px: 1 }}>
          <Box sx={{ display: 'flex', flexWrap: 'nowrap', gap: 1.5 }}>
            <Box sx={{ flex: '1 1 40%', minWidth: 240, alignSelf: 'center' }}>
              <SectionHeader>AUDIO INFO</SectionHeader>
              <Stack spacing={0.5} sx={{ pl: 1.5 }}>
                {Row('Codec', audioCodec, { maxCh: 24 })}
                {Row('Samplerate', audioSampleRate ? `${audioSampleRate} Hz` : '—', {
                  mono: true,
                  maxCh: 24
                })}
                {Row('Channels', audioChannels, { mono: true, maxCh: 8 })}
                {Row('Bit depth', audioBitDepth ? `${audioBitDepth} bit` : '—', {
                  mono: true,
                  maxCh: 12
                })}
              </Stack>
            </Box>

            <Box
              sx={{
                flex: '1 1 60%',
                minWidth: 240,
                height: 'clamp(180px, 30vh, 440px)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                mt: 0.5
              }}
            >
              <FFTSpectrum data={pcmData} />
            </Box>
          </Box>
        </Box>
      </Box>

      {/* Update progress dialog */}
      <Dialog open={upDialogOpen} onClose={() => {}} disableEscapeKeyDown>
        <DialogTitle>Software Update</DialogTitle>
        <DialogContent sx={{ width: 360 }}>
          <Typography sx={{ mb: 1 }}>{phaseText}</Typography>
          <LinearProgress
            variant={pct != null ? 'determinate' : 'indeterminate'}
            value={pct != null ? pct : undefined}
          />
          {pct != null && total > 0 && (
            <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
              {pct}% • {human(received)} / {human(total)}
            </Typography>
          )}
          {error && (
            <Typography variant="body2" sx={{ mt: 1 }} color="error">
              {error}
            </Typography>
          )}
          <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
            The app will relaunch automatically when finished.
          </Typography>
        </DialogContent>
      </Dialog>
    </Box>
  )
}
