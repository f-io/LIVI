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
  TextField,
  Typography
} from '@mui/material'
import { useLiviStore, useStatusStore } from '@store/store'
import { useNetworkStatus } from '@renderer/hooks/useNetworkStatus'
import { fmt, isDongleFwCheckResponse, normalizeBoxInfo } from './utils'
import { DongleFwCheckResponse, FwDialogState, Row } from '@renderer/types'
import { EMPTY_STRING } from '@renderer/constants'
import { useTranslation } from 'react-i18next'

type DevToolsStatus = 'idle' | 'uploading' | 'opening' | 'success' | 'partial' | 'error'
type DevToolsUploadResult = Awaited<ReturnType<typeof window.projection.usb.uploadLiviScripts>>

function dedupe(list: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const item of list) {
    if (seen.has(item)) continue
    seen.add(item)
    out.push(item)
  }
  return out
}

function rankDevUrl(url: string): number {
  if (url.includes('/index.html')) return 0
  if (url.includes('/cgi-bin/server.cgi?action=ls&path=/')) return 1
  return 2
}

function toIndexCandidates(urls: string[]): string[] {
  const hosts = new Set<string>()
  for (const raw of urls) {
    if (!/^https?:\/\//i.test(raw)) continue
    try {
      const u = new URL(raw)
      if (!u.host) continue
      hosts.add(u.host)
    } catch {
      // ignore malformed candidate
    }
  }
  return Array.from(hosts).map((host) => `http://${host}/index.html`)
}

function normalizeIp(raw: string): string {
  return String(raw ?? '').trim()
}

function maskIpv4Input(raw: string): string {
  const cleaned = String(raw ?? '').replace(/[^\d.]/g, '')
  const parts = cleaned.split('.').slice(0, 4)
  return parts
    .map((p) => p.replace(/\D/g, '').slice(0, 3))
    .join('.')
    .slice(0, 15)
}

function isValidIpv4(raw: string): boolean {
  const ip = normalizeIp(raw)
  if (!ip) return false
  const parts = ip.split('.')
  if (parts.length !== 4) return false
  return parts.every((p) => {
    if (!/^\d+$/.test(p)) return false
    const n = Number(p)
    return n >= 0 && n <= 255
  })
}

export function USBDongle() {
  const { t } = useTranslation()
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)

  // Network status
  const network = useNetworkStatus()
  const isOnline = network.online

  // USB descriptor
  const vendorId = useLiviStore((s) => s.vendorId)
  const productId = useLiviStore((s) => s.productId)
  const usbFwVersion = useLiviStore((s) => s.usbFwVersion)

  // Dongle Info
  const dongleFwVersion = useLiviStore((s) => s.dongleFwVersion)
  const boxInfoRaw = useLiviStore((s) => s.boxInfo)

  // Video stream (negotiated)
  const negotiatedWidth = useLiviStore((s) => s.negotiatedWidth)
  const negotiatedHeight = useLiviStore((s) => s.negotiatedHeight)

  // Audio stream
  const audioCodec = useLiviStore((s) => s.audioCodec)
  const audioSampleRate = useLiviStore((s) => s.audioSampleRate)
  const audioChannels = useLiviStore((s) => s.audioChannels)
  const audioBitDepth = useLiviStore((s) => s.audioBitDepth)

  // Auto-close dialog
  const autoCloseTimerRef = useRef<number | null>(null)
  const [fwWaitingForReconnect, setFwWaitingForReconnect] = useState(false)
  const [fwSawDisconnect, setFwSawDisconnect] = useState(false)

  // Dev tools
  const [devStatus, setDevStatus] = useState<DevToolsStatus>('idle')
  const [devResult, setDevResult] = useState<DevToolsUploadResult | null>(null)
  const [devError, setDevError] = useState<string | null>(null)
  const [devOpenedUrl, setDevOpenedUrl] = useState<string | null>(null)
  const [devLog, setDevLog] = useState<string[]>([])
  const [devIpInput, setDevIpInput] = useState('')
  const [devIpFocused, setDevIpFocused] = useState(false)

  // Parsed box info
  const boxInfo = useMemo(() => normalizeBoxInfo(boxInfoRaw), [boxInfoRaw])
  const devList = useMemo(
    () => (Array.isArray(boxInfo?.DevList) ? (boxInfo?.DevList ?? []) : []),
    [boxInfo]
  )

  const resolution =
    negotiatedWidth && negotiatedHeight ? `${negotiatedWidth}×${negotiatedHeight}` : EMPTY_STRING

  const audioLine = useMemo(() => {
    const parts: string[] = []
    if (audioCodec) parts.push(String(audioCodec))
    if (audioSampleRate) parts.push(`${audioSampleRate} Hz`)
    if (audioChannels != null) parts.push(`${audioChannels} ch`)
    if (audioBitDepth) parts.push(`${audioBitDepth} bit`)
    return parts.length ? parts.join(' • ') : EMPTY_STRING
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
    latestVer !== EMPTY_STRING &&
    dongleVer.length > 0 &&
    latestVer !== dongleVer

  // Show vendor "latest" version if we have one.
  // Vendor semantics: "-" means "already latest" -> show current dongle version to keep UI stable.
  const latestFwLabel = ok
    ? latestVer && latestVer !== EMPTY_STRING
      ? latestVer
      : dongleVer || EMPTY_STRING
    : EMPTY_STRING

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
    if (!fwResult) return EMPTY_STRING

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

  const devBusy = devStatus === 'uploading' || devStatus === 'opening'
  const canEnableDevTools = !devBusy && Boolean(isDongleConnected)

  const pushDevLog = useCallback((line: string) => {
    const ts = new Date().toISOString().slice(11, 19)
    setDevLog((prev) => [...prev, `[${ts}] ${line}`].slice(-10))
  }, [])

  const devUrlCandidates = useMemo(() => {
    const urls = dedupe(devResult?.urls ?? [])
    return urls.sort((a, b) => rankDevUrl(a) - rankDevUrl(b))
  }, [devResult])

  useEffect(() => {
    setDevIpInput(normalizeIp(settings?.dongleToolsIp ?? ''))
  }, [settings?.dongleToolsIp])

  const handleEnableDevTools = useCallback(async () => {
    if (devBusy) return
    setDevError(null)
    setDevResult(null)
    setDevOpenedUrl(null)
    setDevLog([])

    try {
      setDevStatus('uploading')
      pushDevLog(t('settings.devToolsLogUploadStart'))
      const configuredIp = normalizeIp(devIpInput)

      if (configuredIp && !isValidIpv4(configuredIp)) {
        throw new Error(t('settings.devToolsInvalidIp', { ip: configuredIp }))
      }

      await saveSettings({ dongleToolsIp: configuredIp })
      if (configuredIp) {
        pushDevLog(t('settings.devToolsLogUsingIp', { ip: configuredIp }))
      } else {
        pushDevLog(t('settings.devToolsLogUsingAutoCandidates'))
      }

      const res = await window.projection.usb.uploadLiviScripts()
      setDevResult(res)
      pushDevLog(
        t('settings.devToolsLogUploadDone', {
          ok: String(res.ok),
          cgiOk: String(res.cgiOk),
          webOk: String(res.webOk),
          durationMs: res.durationMs
        })
      )

      if (!res.ok) {
        setDevStatus('partial')
        setDevError(
          t('settings.devToolsPartial', {
            cgiOk: String(res.cgiOk),
            webOk: String(res.webOk)
          })
        )
        return
      }

      const openTargets = configuredIp
        ? [`http://${configuredIp}/index.html`]
        : dedupe(toIndexCandidates(res.urls ?? [])).sort((a, b) => rankDevUrl(a) - rankDevUrl(b))
      if (openTargets.length === 0) {
        setDevStatus('success')
        pushDevLog(t('settings.devToolsLogNoCandidates'))
        return
      }

      setDevStatus('opening')
      pushDevLog(t('settings.devToolsLogOpeningCount', { count: openTargets.length }))

      let openedCount = 0
      let firstOpened: string | null = null
      for (const url of openTargets) {
        const openRes = await window.app.openExternal(url)
        if (openRes?.ok) {
          openedCount += 1
          firstOpened ??= url
        } else {
          pushDevLog(
            t('settings.devToolsLogOpenFailed', {
              url,
              error: openRes?.error || t('settings.devToolsUnknownError')
            })
          )
        }
      }

      if (openedCount === 0) {
        throw new Error(t('settings.devToolsOpenNoneFailed'))
      }

      setDevOpenedUrl(firstOpened)
      setDevStatus('success')
      pushDevLog(
        t('settings.devToolsLogOpenDone', { opened: openedCount, total: openTargets.length })
      )
    } catch (e) {
      setDevStatus('error')
      setDevError(e instanceof Error ? e.message : String(e))
      pushDevLog(`Error: ${e instanceof Error ? e.message : String(e)}`)
    }
  }, [devBusy, devIpInput, pushDevLog, saveSettings, t])

  useEffect(() => {
    if (isDongleConnected) return
    setDevStatus('idle')
    setDevResult(null)
    setDevOpenedUrl(null)
    setDevError(null)
    setDevLog([])
  }, [isDongleConnected])

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

  function isRecord(v: unknown): v is Record<string, unknown> {
    return typeof v === 'object' && v !== null
  }

  // Listen to main-process fwUpdate events
  useEffect(() => {
    const handler = (_event: unknown, payload: unknown) => {
      if (!isRecord(payload)) return
      if (payload.type !== 'fwUpdate') return

      const stageRaw = payload.stage
      const stage = typeof stageRaw === 'string' ? stageRaw : String(stageRaw ?? '')
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

    window.projection?.ipc?.onEvent?.(handler)
    return () => {
      window.projection?.ipc?.offEvent?.(handler)
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
            const st = await window.projection.ipc.dongleFirmware('status')
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
        const raw = await window.projection.ipc.dongleFirmware(action)

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
      { label: 'WiFi Channel', value: boxInfo?.wifiChannel ?? null, mono: true },
      { label: 'Links', value: fmt(boxInfo?.supportLinkType) },
      { label: 'Features', value: fmt(boxInfo?.supportFeatures) },
      { label: 'CusCode', value: fmt(boxInfo?.CusCode), mono: true },
      { label: 'Channel List', value: fmt(boxInfo?.ChannelList), mono: true }
    ],
    [boxInfo]
  )

  const rowsPhoneInfo = useMemo<Row[]>(
    () => [
      { label: 'Link Type', value: fmt(boxInfo?.MDLinkType) },
      { label: 'Model', value: fmt(boxInfo?.MDModel) },
      { label: 'OS', value: fmt(boxInfo?.MDOSVersion), mono: true },
      { label: 'Link Version', value: fmt(boxInfo?.MDLinkVersion), mono: true },
      { label: 'BT Name', value: fmt(boxInfo?.btName) },
      { label: 'BT MAC', value: fmt(boxInfo?.btMacAddr), mono: true },
      { label: 'CPU Temp', value: boxInfo?.cpuTemp ?? null, mono: true }
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

  const renderDeviceList = () => {
    if (devList.length === 0) {
      return renderRows([{ label: 'Device List', value: '—' }])
    }

    return (
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
              tabIndex={0}
              role="group"
              aria-label={`Device ${idx}`}
              sx={{
                px: 1,
                py: 0.75,
                borderRadius: 1.25,
                outline: 'none',
                '&:focus-visible': {
                  bgcolor: 'action.selected'
                }
              }}
            >
              <Typography sx={{ mb: 0.5 }}>Device {idx}:</Typography>

              <Stack spacing={0.5} sx={{ pl: 2 }}>
                <Box sx={{ display: 'flex', gap: 1 }}>
                  <Typography sx={{ minWidth: 120 }} color="text.secondary">
                    Name:
                  </Typography>
                  <Typography sx={{ ...Mono }}>{name}</Typography>
                </Box>

                <Box sx={{ display: 'flex', gap: 1 }}>
                  <Typography sx={{ minWidth: 120 }} color="text.secondary">
                    Type:
                  </Typography>
                  <Typography sx={{ ...Mono }}>{type}</Typography>
                </Box>

                <Box sx={{ display: 'flex', gap: 1 }}>
                  <Typography sx={{ minWidth: 120 }} color="text.secondary">
                    ID:
                  </Typography>
                  <Typography sx={{ ...Mono }}>{id}</Typography>
                </Box>

                <Box sx={{ display: 'flex', gap: 1 }}>
                  <Typography sx={{ minWidth: 120 }} color="text.secondary">
                    Time:
                  </Typography>
                  <Typography sx={{ ...Mono }}>{time}</Typography>
                </Box>

                <Box sx={{ display: 'flex', gap: 1 }}>
                  <Typography sx={{ minWidth: 120 }} color="text.secondary">
                    RFCOMM:
                  </Typography>
                  <Typography sx={{ ...Mono }}>{rfcomm}</Typography>
                </Box>
              </Stack>
            </Box>
          )
        })}
      </Stack>
    )
  }

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
        Status
      </Typography>
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

      <Divider sx={{ my: 1.5 }} />

      <Typography variant="subtitle2" color="text.secondary">
        Phone
      </Typography>

      {renderRows(rowsPhoneInfo)}

      <Divider />

      {renderDeviceList()}

      <Divider />

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

      <Typography variant="subtitle2" color="text.secondary">
        {`${t('settings.devTools')} (${t('settings.mustBeOnDongleWifi')})`}
      </Typography>

      <Stack
        direction="row"
        spacing={1}
        sx={{ alignItems: 'flex-start', flexWrap: 'wrap', px: 1, mb: 1 }}
      >
        <Button
          variant="outlined"
          size="small"
          disabled={!canEnableDevTools}
          onClick={handleEnableDevTools}
          sx={{ height: 40 }}
        >
          {devBusy ? (
            <Box sx={{ display: 'inline-flex', alignItems: 'center', gap: 1 }}>
              <CircularProgress size={14} />
              {devStatus === 'opening' ? t('settings.opening') : t('settings.enabling')}
            </Box>
          ) : (
            t('settings.enableDevTools')
          )}
        </Button>
        <TextField
          label={t('settings.dongleIpOptional')}
          placeholder={
            devIpFocused || devIpInput.length > 0
              ? t('settings.dongleIpMaskPlaceholder')
              : t('settings.dongleIpPlaceholder')
          }
          size="small"
          sx={{ minWidth: 240, flex: '1 1 280px' }}
          value={devIpInput}
          disabled={devBusy}
          onFocus={() => setDevIpFocused(true)}
          onBlur={() => setDevIpFocused(false)}
          onChange={(e) => setDevIpInput(maskIpv4Input(e.target.value))}
          error={devIpInput.length > 0 && !isValidIpv4(devIpInput)}
          inputProps={{ inputMode: 'numeric', maxLength: 15 }}
          InputProps={{ sx: { height: 40 } }}
          helperText={
            devIpInput.length > 0 && !isValidIpv4(devIpInput) ? t('settings.enterValidIpv4') : ' '
          }
        />
      </Stack>
      {devBusy ? <LinearProgress sx={{ mt: 1 }} /> : null}
      {devError ? (
        <Alert severity="error" sx={{ mt: 1 }}>
          {devError}
        </Alert>
      ) : devResult ? (
        <Alert severity={devResult.ok ? 'success' : 'warning'} sx={{ mt: 1 }}>
          {devResult.ok
            ? t('settings.devToolsEnabled')
            : t('settings.devToolsPartial', {
                cgiOk: String(devResult.cgiOk),
                webOk: String(devResult.webOk)
              })}
        </Alert>
      ) : null}
      {!devOpenedUrl && devUrlCandidates.length > 0 ? (
        <Alert severity="info" sx={{ mt: 1 }}>
          {t('settings.tryOneOfUrls')}
          <Box component="ul" sx={{ m: 0, pl: 2 }}>
            {devUrlCandidates.slice(0, 6).map((url) => (
              <li key={url}>
                <a href={url} target="_blank" rel="noreferrer">
                  {url}
                </a>
              </li>
            ))}
          </Box>
        </Alert>
      ) : null}
      {devLog.length > 0 ? (
        <Box sx={{ mt: 1, px: 1 }}>
          <Typography variant="caption" color="text.secondary">
            {t('settings.devToolsLog')}
          </Typography>
          <Box
            component="pre"
            sx={{
              m: 0,
              p: 1,
              border: '1px solid',
              borderColor: 'divider',
              borderRadius: 1,
              background: 'rgba(0,0,0,0.25)',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              ...Mono
            }}
          >
            {devLog.join('\n')}
          </Box>
        </Box>
      ) : null}
    </Box>
  )
}
