import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  LinearProgress,
  Stack,
  Typography
} from '@mui/material'
import { useCarplayStore, useStatusStore } from '@store/store'
import { useNetworkStatus } from '../../../../../hooks/useNetworkStatus'

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

  if (typeof input === 'object') {
    return input as BoxInfoPayload
  }

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

type DongleFwApiRaw = {
  err: number
  token?: string
  ver?: string
  size?: string | number
  id?: string
  notes?: string
  msg?: string
  error?: string
}

type LocalFwStatus =
  | { ok: true; ready: true; path: string; bytes: number; model: string; latestVer?: string }
  | { ok: true; ready: false; reason: string }
  | { ok: false; error: string }

type DongleFwCheckResponse = {
  ok: boolean
  hasUpdate: boolean
  size: string | number
  token?: string
  request?: Record<string, unknown> & { local?: LocalFwStatus }
  raw: DongleFwApiRaw
  error?: string
}

function isDongleFwCheckResponse(v: unknown): v is DongleFwCheckResponse {
  if (!v || typeof v !== 'object') return false
  const o = v as any
  return typeof o.ok === 'boolean' && o.raw && typeof o.raw === 'object' && 'err' in o.raw
}

type FwPhase = 'start' | 'download' | 'ready' | 'error' | 'upload'

type FwProgress = {
  percent?: number
  received?: number
  total?: number
}

type FwDialogState = {
  open: boolean
  phase: FwPhase
  progress: FwProgress
  error: string
  message: string
  inFlight: boolean
}

export function USBDongle() {
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const isStreaming = useStatusStore((s) => s.isStreaming)

  // Network status
  const network = useNetworkStatus()
  const isOnline = network.online

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

  // Auto-close dialog
  const autoCloseTimerRef = useRef<number | null>(null)
  const [fwWaitingForReconnect, setFwWaitingForReconnect] = useState(false)
  const [fwSawDisconnect, setFwSawDisconnect] = useState(false)

  // Parsed box info
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

  // Dongle FW check/update UI state
  const [fwBusy, setFwBusy] = useState<null | 'status' | 'check' | 'download' | 'upload'>(null)
  const [fwResult, setFwResult] = useState<DongleFwCheckResponse | null>(null)
  const [fwUiError, setFwUiError] = useState<string | null>(null)

  // "SoftwareUpdate-like" dialog state for dongle firmware download/upload
  const [fwDlg, setFwDlg] = useState<FwDialogState>({
    open: false,
    phase: 'start',
    progress: {},
    error: '',
    message: '',
    inFlight: false
  })

  // Changelog dialog UI state
  const [changelogOpen, setChangelogOpen] = useState(false)

  const ok = Boolean(fwResult?.ok) && fwResult?.raw?.err === 0

  // Vendor/API version string
  const latestVer = typeof fwResult?.raw?.ver === 'string' ? fwResult.raw.ver.trim() : ''

  // Device version string
  const dongleVer = typeof dongleFwVersion === 'string' ? dongleFwVersion.trim() : ''

  const notes = typeof fwResult?.raw?.notes === 'string' ? fwResult.raw.notes : undefined
  const hasChangelog = typeof notes === 'string' && notes.trim().length > 0

  // Vendor semantics: "-" means "already latest"
  const apiSaysNoUpdate = ok && latestVer === '-'

  // UI-trustworthy update flag: only if vendor version is different from the dongle version
  const hasUpdate =
    ok &&
    latestVer.length > 0 &&
    latestVer !== '-' &&
    dongleVer.length > 0 &&
    latestVer !== dongleVer

  // Show vendor "latest" version if we have one.
  // Vendor semantics: "-" means "already latest" -> show current dongle version to keep UI stable.
  const latestFwLabel = ok ? (latestVer && latestVer !== '-' ? latestVer : dongleVer || '—') : '—'

  // Local firmware status (manifest-based)
  const local = fwResult?.request?.local

  const localReady = local?.ok === true && local.ready === true
  const localPath = localReady ? local.path : ''
  const localBytes = localReady ? local.bytes : 0
  const localReason =
    local && local.ok === true && local.ready === false
      ? local.reason
      : local && local.ok === false
        ? local.error
        : ''

  // Versions for "same as dongle" check (local package version vs dongle)
  const localLatestVer =
    localReady && typeof local.latestVer === 'string' ? local.latestVer.trim() : ''

  const localIsSameAsDongle =
    localReady && localLatestVer.length > 0 && dongleVer.length > 0 && localLatestVer === dongleVer

  const vendorSaysUpdate = hasUpdate && !apiSaysNoUpdate
  const shouldOfferUpload = localReady && !localIsSameAsDongle && vendorSaysUpdate

  const safePreview = (v: unknown): string => {
    try {
      if (v == null) return String(v)
      if (typeof v === 'string') return v
      if (typeof v === 'number' || typeof v === 'boolean') return String(v)

      const json = JSON.stringify(
        v,
        (_k, val) =>
          typeof val === 'string' && val.length > 2000 ? val.slice(0, 2000) + '…' : val,
        2
      )

      return json.length > 6000 ? json.slice(0, 6000) + '\n…(truncated)…' : json
    } catch (e) {
      return `<<unstringifiable: ${e instanceof Error ? e.message : String(e)}>>`
    }
  }

  const human = (n: number) =>
    n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`

  const fwStatusText = useMemo(() => {
    if (fwBusy === 'status') return 'Checking local status…'
    if (fwBusy === 'check') return 'Checking…'
    if (fwBusy === 'download') return 'Downloading…'
    if (fwBusy === 'upload') return 'Uploading…'
    if (!fwResult) return '—'

    // 1) API errors
    if (!fwResult.ok || fwResult.raw?.err !== 0) {
      const msg = fwResult.raw?.msg || fwResult.raw?.error || fwResult.error || 'Unknown error'
      return `Error: ${String(msg)}`
    }

    // 2) update availability
    if (hasUpdate) return 'Update available'

    // 3) no update -> ALWAYS show "Up to date"
    return 'Up to date'
  }, [fwBusy, fwResult, hasUpdate])

  const canCheck =
    fwBusy == null &&
    isOnline &&
    Boolean(isDongleConnected) &&
    Boolean(fmt(dongleFwVersion)) &&
    Boolean(fmt(boxInfo?.uuid)) &&
    Boolean(fmt(boxInfo?.MFD)) &&
    Boolean(fmt(boxInfo?.productType))

  const canDownload = fwBusy == null && Boolean(isDongleConnected) && hasUpdate && isOnline

  const canUpload = fwBusy == null && Boolean(isDongleConnected) && shouldOfferUpload

  const fwPct = useMemo(() => {
    const p = fwDlg.progress?.percent
    if (typeof p !== 'number' || !Number.isFinite(p)) return null
    const clamped = Math.max(0, Math.min(1, p))
    return Math.round(clamped * 100)
  }, [fwDlg.progress])

  const fwPhaseText =
    fwDlg.phase === 'download'
      ? 'Downloading'
      : fwDlg.phase === 'upload'
        ? 'Uploading'
        : fwDlg.phase === 'ready'
          ? 'Done'
          : fwDlg.phase === 'start'
            ? 'Starting…'
            : fwDlg.phase === 'error'
              ? 'Error'
              : 'Working…'

  const clearAutoCloseTimer = useCallback(() => {
    if (autoCloseTimerRef.current != null) {
      window.clearTimeout(autoCloseTimerRef.current)
      autoCloseTimerRef.current = null
    }
  }, [])

  const closeFwDialog = useCallback(() => {
    clearAutoCloseTimer()
    setFwWaitingForReconnect(false)
    setFwSawDisconnect(false)

    setFwDlg({
      open: false,
      phase: 'start',
      progress: {},
      error: '',
      message: '',
      inFlight: false
    })
  }, [clearAutoCloseTimer])

  // Listen to main-process fwUpdate events
  useEffect(() => {
    const handler = (_event: unknown, payload: any) => {
      if (!payload || typeof payload !== 'object') return
      if (payload.type !== 'fwUpdate') return

      const stage = String(payload.stage || '')
      if (!stage) return

      // download
      if (stage === 'download:start') {
        setFwDlg({
          open: true,
          phase: 'start',
          progress: {},
          error: '',
          message: '',
          inFlight: true
        })
        return
      }
      if (stage === 'download:progress') {
        const received = typeof payload.received === 'number' ? payload.received : 0
        const total = typeof payload.total === 'number' ? payload.total : 0
        const percent =
          typeof payload.percent === 'number'
            ? payload.percent
            : total > 0
              ? received / total
              : undefined

        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'download',
          inFlight: true,
          error: '',
          message: prev.message || 'Download in progress…',
          progress: { received, total, percent }
        }))
        return
      }
      if (stage === 'download:done') {
        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'ready',
          inFlight: false,
          error: '',
          message: payload.path ? `Saved to: ${String(payload.path)}` : 'Saved'
        }))
        return
      }
      if (stage === 'download:error') {
        const msg = payload.message ? String(payload.message) : 'Download failed'
        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'error',
          inFlight: false,
          error: msg,
          message: msg
        }))
        return
      }

      // upload
      if (stage === 'upload:start') {
        setFwDlg({
          open: true,
          phase: 'start',
          progress: {},
          error: '',
          message: '',
          inFlight: true
        })
        return
      }
      if (stage === 'upload:progress') {
        // Option A: int32 progress (0..100) coming from 0xb1
        const pInt = typeof payload.progress === 'number' ? payload.progress : undefined
        const percentFromInt =
          typeof pInt === 'number' && Number.isFinite(pInt)
            ? Math.max(0, Math.min(1, pInt / 100))
            : undefined

        // Option B: byte progress (if you later add sent/total)
        const received = typeof payload.sent === 'number' ? payload.sent : 0
        const total = typeof payload.total === 'number' ? payload.total : 0
        const percentFromBytes =
          typeof payload.percent === 'number'
            ? payload.percent
            : total > 0
              ? received / total
              : undefined

        const percent = percentFromInt ?? percentFromBytes

        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'upload',
          inFlight: true,
          error: '',
          message: prev.message || 'Update in progress…',
          progress: { received, total, percent }
        }))
        return
      }
      if (stage === 'upload:state') {
        const statusText = payload.statusText ? String(payload.statusText) : ''
        const isTerminal = Boolean(payload.isTerminal)
        const ok = Boolean(payload.ok)

        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: isTerminal ? (ok ? 'ready' : 'error') : 'upload',
          inFlight: !isTerminal,
          error: isTerminal && !ok ? statusText || 'Update failed' : '',
          message: statusText || prev.message || 'Update in progress…'
        }))

        return
      }
      if (stage === 'upload:done') {
        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'ready',
          inFlight: false,
          error: '',
          message: payload.message ? String(payload.message) : 'Upload complete'
        }))
        return
      }
      if (stage === 'upload:error') {
        const msg = payload.message ? String(payload.message) : 'Upload failed'
        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'error',
          inFlight: false,
          error: msg,
          message: msg
        }))
        return
      }
    }

    window.carplay?.ipc?.onEvent?.(handler)
    return () => {
      window.carplay?.ipc?.offEvent?.(handler)
    }
  }, [])

  useEffect(() => {
    if (!fwDlg.open) return

    // Cancel any pending auto-close when state changes
    if (autoCloseTimerRef.current != null) {
      window.clearTimeout(autoCloseTimerRef.current)
      autoCloseTimerRef.current = null
    }

    // Upload reconnect auto-close:
    // Only close after we reached a terminal "ready" state AND we saw a disconnect->reconnect.
    if (fwWaitingForReconnect) {
      const uploadFinished = fwDlg.phase === 'ready' && !fwDlg.inFlight && !fwDlg.error

      if (!isDongleConnected) {
        if (!fwSawDisconnect) setFwSawDisconnect(true)
        return
      }

      if (uploadFinished && fwSawDisconnect && isDongleConnected) {
        setFwWaitingForReconnect(false)
        setFwSawDisconnect(false)
        closeFwDialog()
      }

      return
    }

    // Download auto-close:
    const isDownloadDone =
      fwDlg.phase === 'ready' &&
      !fwDlg.inFlight &&
      !fwDlg.error &&
      typeof fwDlg.message === 'string' &&
      (fwDlg.message.startsWith('Saved') || fwDlg.message.startsWith('Already downloaded'))

    if (!isDownloadDone) return

    autoCloseTimerRef.current = window.setTimeout(() => {
      closeFwDialog()
      autoCloseTimerRef.current = null
    }, 900)

    return () => {
      if (autoCloseTimerRef.current != null) {
        window.clearTimeout(autoCloseTimerRef.current)
        autoCloseTimerRef.current = null
      }
    }
  }, [
    fwDlg.open,
    fwDlg.phase,
    fwDlg.inFlight,
    fwDlg.error,
    fwDlg.message,
    fwWaitingForReconnect,
    fwSawDisconnect,
    isDongleConnected,
    closeFwDialog
  ])

  const handleFwAction = useCallback(
    async (action: 'status' | 'check' | 'download' | 'upload') => {
      setFwUiError(null)

      try {
        setFwBusy(action)

        // Preflight for download: if already localReady -> just show dialog message and exit
        if (action === 'download') {
          try {
            const st = await window.carplay.ipc.dongleFirmware('status')
            if (isDongleFwCheckResponse(st)) {
              setFwResult((prev) => {
                if (!prev) return st
                return {
                  ...prev,
                  request: {
                    ...(prev.request ?? {}),
                    ...(st.request ?? {})
                  }
                }
              })

              const stLocal = st.request?.local
              const ready = stLocal?.ok === true && stLocal.ready === true

              if (st.ok && st.raw?.err === 0 && ready) {
                setFwDlg({
                  open: true,
                  phase: 'ready',
                  progress: {},
                  error: '',
                  message: stLocal.path
                    ? `Already downloaded.\nSaved to: ${String(stLocal.path)}`
                    : 'Already downloaded.',
                  inFlight: false
                })
                return
              }
            }
          } catch {
            // ignore status preflight errors; we can still attempt download
          }

          setFwDlg({
            open: true,
            phase: 'start',
            progress: {},
            error: '',
            message: '',
            inFlight: true
          })
        }

        if (action === 'upload') {
          // UI safety: never upload unless localReady is true
          if (!localReady) {
            setFwUiError(localReason || 'Local firmware is not ready for this dongle.')
            return
          }

          // Wait for dongle reboot/reconnect after upload
          setFwWaitingForReconnect(true)
          setFwSawDisconnect(false)

          // Start dialog even if main doesn't emit progress yet
          setFwDlg({
            open: true,
            phase: 'start',
            progress: {},
            error: '',
            message: localPath ? `Using: ${localPath}` : '',
            inFlight: true
          })
        }

        // Call preload -> main IPC
        const raw = await window.carplay.ipc.dongleFirmware(action)
        console.log('[DongleInfo] dongleFirmware raw =', raw)

        if (!isDongleFwCheckResponse(raw)) {
          setFwResult(null)
          setFwUiError(
            `Invalid response from main process (type=${typeof raw})\n\nRAW:\n${safePreview(raw)}`
          )
          setFwDlg((prev) => ({
            ...prev,
            open: true,
            phase: 'error',
            inFlight: false,
            error: 'Invalid response from main process',
            message: 'Invalid response from main process'
          }))
          return
        }

        setFwResult((prev) => {
          if (action === 'status') {
            if (!prev) return raw

            return {
              ...prev,
              request: {
                ...(prev.request ?? {}),
                ...(raw.request ?? {})
              },
              raw: {
                ...prev.raw,
                msg: raw.raw?.msg ?? prev.raw?.msg
              }
            }
          }
          return raw
        })

        if (!raw.ok || raw.raw?.err !== 0) {
          const msg = raw.raw?.msg || raw.raw?.error || raw.error || 'Unknown error'
          setFwUiError(String(msg))
          setFwDlg((prev) => ({
            ...prev,
            open: true,
            phase: 'error',
            inFlight: false,
            error: String(msg),
            message: String(msg)
          }))
          return
        }

        if (action === 'upload') {
          // Keep dialog open until we receive upload:done / upload:error from dongle
          setFwDlg((prev) => ({
            ...prev,
            open: true,
            phase: 'upload',
            inFlight: true,
            error: '',
            message: 'Update in progress…'
          }))
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        setFwUiError(msg)
        setFwResult(null)

        setFwDlg((prev) => ({
          ...prev,
          open: true,
          phase: 'error',
          inFlight: false,
          error: msg,
          message: msg
        }))
      } finally {
        setFwBusy(null)
      }
    },
    [localReady, localReason, localPath]
  )

  useEffect(() => {
    // Pull local manifest status whenever the dongle becomes available
    if (!isDongleConnected) return
    if (!fmt(dongleFwVersion) || !fmt(boxInfo?.uuid)) return

    handleFwAction('status').catch(() => {})
  }, [isDongleConnected, dongleFwVersion, boxInfo?.uuid, handleFwAction])

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

  const localLabel = useMemo(() => {
    if (!fwResult) return '—'
    if (localReady) return `Ready • ${human(localBytes)}`
    if (localReason) return localReason
    return 'Not ready'
  }, [fwResult, localReady, localBytes, localReason])

  const rowsFw = useMemo<Row[]>(
    () => [
      { label: 'Dongle FW', value: dongleFwVersion, mono: true },
      { label: 'Latest FW', value: latestFwLabel, mono: true },
      { label: 'FW Status', value: fwStatusText, mono: true },
      {
        label: 'Local FW',
        value: localLabel,
        mono: true,
        tooltip: localReady ? localPath : localReason
      }
    ],
    [dongleFwVersion, latestFwLabel, fwStatusText, localLabel, localReady, localPath, localReason]
  )

  const rowsDongleInfo = useMemo<Row[]>(
    () => [
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
    [boxInfo]
  )

  const rowsStreams = useMemo<Row[]>(
    () => [
      { label: 'Resolution', value: resolution, mono: true },
      { label: 'Audio', value: audioLine, mono: true }
    ],
    [resolution, audioLine]
  )

  const renderRows = (rows: Row[]) => (
    <Stack spacing={0.5}>
      {rows.map((r) => {
        const text = r.value != null && String(r.value).trim() ? String(r.value) : '—'
        const title = r.tooltip ?? (typeof r.value === 'string' ? r.value : '')

        return (
          <Box
            key={r.label}
            tabIndex={0}
            role="listitem"
            aria-label={`${r.label}: ${text}`}
            sx={{
              display: 'flex',
              alignItems: 'baseline',
              gap: 1,
              px: 1,
              py: 0.75,
              borderRadius: 1.25,
              outline: 'none',
              '&:focus-visible': {
                bgcolor: 'action.selected'
              }
            }}
          >
            <Typography sx={{ minWidth: 140 }} color="text.secondary">
              {r.label}:
            </Typography>

            <Typography
              title={title}
              sx={{
                ...(r.mono ? Mono : null),
                whiteSpace: 'nowrap',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                maxWidth: '52ch'
              }}
            >
              {text}
            </Typography>
          </Box>
        )
      })}
    </Stack>
  )

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Typography variant="subtitle2" color="text.secondary">
        Firmware
      </Typography>

      {renderRows(rowsFw)}

      {/* FW actions */}
      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', flexWrap: 'wrap', px: 1 }}>
        <Button
          variant="outlined"
          size="small"
          disabled={!canCheck}
          onClick={() => handleFwAction('check')}
        >
          {fwBusy === 'check' ? (
            <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 1 }}>
              <CircularProgress size={14} />
              Check for Updates
            </Box>
          ) : (
            'Check for Updates'
          )}
        </Button>

        <Button
          variant="contained"
          size="small"
          disabled={!canDownload}
          onClick={() => handleFwAction('download')}
        >
          {fwBusy === 'download' ? (
            <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 1 }}>
              <CircularProgress size={14} />
              Download
            </Box>
          ) : (
            'Download'
          )}
        </Button>

        <Button
          variant="contained"
          size="small"
          disabled={!canUpload}
          onClick={() => handleFwAction('upload')}
          title={
            !canUpload
              ? localReason || 'No matching firmware downloaded for this dongle.'
              : localPath
          }
        >
          {fwBusy === 'upload' ? (
            <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 1 }}>
              <CircularProgress size={14} />
              Upload
            </Box>
          ) : (
            'Upload'
          )}
        </Button>

        <Button
          variant="text"
          size="small"
          disabled={!hasChangelog}
          onClick={() => setChangelogOpen(true)}
        >
          Changelog
        </Button>
      </Stack>

      {/* Progress dialog (download/upload) */}
      <Dialog open={fwDlg.open} onClose={() => {}} disableEscapeKeyDown>
        <DialogTitle>Dongle Firmware</DialogTitle>
        <DialogContent sx={{ width: 360 }}>
          <Typography sx={{ mb: 1 }}>{fwPhaseText}</Typography>

          <LinearProgress
            variant={fwPct != null ? 'determinate' : 'indeterminate'}
            value={fwPct != null ? fwPct : undefined}
          />

          {fwPct != null && (fwDlg.progress.total ?? 0) > 0 && (
            <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
              {fwPct}% • {human(fwDlg.progress.received ?? 0)} / {human(fwDlg.progress.total ?? 0)}
            </Typography>
          )}

          {fwDlg.error && (
            <Typography variant="body2" sx={{ mt: 1 }} color="error">
              {fwDlg.error}
            </Typography>
          )}

          {!fwDlg.error && fwDlg.message && (
            <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
              {fwDlg.message}
            </Typography>
          )}
        </DialogContent>

        <DialogActions>
          {fwDlg.phase === 'error' && (
            <Button variant="outlined" onClick={closeFwDialog}>
              Close
            </Button>
          )}
        </DialogActions>
      </Dialog>

      {/* Changelog dialog (vendor-provided, optional) */}
      <Dialog open={changelogOpen} onClose={() => setChangelogOpen(false)} fullWidth maxWidth="sm">
        <DialogTitle>Vendor changelog</DialogTitle>
        <DialogContent dividers>
          <Typography variant="body2" color="text.secondary" sx={{ mb: 1 }}>
            This information is provided by the dongle vendor and may be incomplete or untranslated.
          </Typography>

          <Typography variant="body2" sx={{ ...Mono, whiteSpace: 'pre-wrap' }}>
            {notes?.trim() || '—'}
          </Typography>
        </DialogContent>
        <DialogActions>
          <Button variant="text" onClick={() => setChangelogOpen(false)}>
            Close
          </Button>
        </DialogActions>
      </Dialog>

      {fwUiError ? (
        <Alert severity="error" sx={{ mt: 1 }}>
          {fwUiError}
        </Alert>
      ) : null}

      <Divider />

      {renderRows(rowsTop)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        Streams
      </Typography>
      {renderRows(rowsStreams)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        USB Descriptor
      </Typography>
      {renderRows(rowsUsb)}

      <Divider />

      <Typography variant="subtitle2" color="text.secondary">
        Dongle Info
      </Typography>

      {renderRows(rowsDongleInfo)}

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
