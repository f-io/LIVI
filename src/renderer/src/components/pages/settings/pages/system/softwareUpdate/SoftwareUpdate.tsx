import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Box,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  LinearProgress,
  Stack,
  Typography
} from '@mui/material'
import { cmpSemver, parseSemver } from './utils'
import { phaseMap, UpdatePhase, UpgradeText } from './types'
import { EMPTY_STRING } from '@renderer/constants'

export function SoftwareUpdate() {
  const [installedVersion, setInstalledVersion] = useState<string>(EMPTY_STRING)
  const [latestVersion, setLatestVersion] = useState<string>(EMPTY_STRING)
  const [latestUrl, setLatestUrl] = useState<string | undefined>(undefined)

  const [message, setMessage] = useState<string>('')

  const [upDialogOpen, setUpDialogOpen] = useState(false)
  const [phase, setPhase] = useState<UpdatePhase>('start')
  const [percent, setPercent] = useState<number | null>(null)
  const [received, setReceived] = useState<number>(0)
  const [total, setTotal] = useState<number>(0)
  const [error, setError] = useState<string>('')

  const [inFlight, setInFlight] = useState(false)

  const installedSem = useMemo(() => parseSemver(installedVersion), [installedVersion])
  const latestSem = useMemo(() => parseSemver(latestVersion), [latestVersion])

  const hasLatest = !!latestUrl && !!latestSem
  const cmp = useMemo(
    () => (installedSem && latestSem ? cmpSemver(installedSem, latestSem) : null),
    [installedSem, latestSem]
  )

  const isDowngrade = hasLatest && cmp !== null && cmp > 0

  const pct = percent != null ? Math.round(percent * 100) : null
  const human = (n: number) =>
    n >= 1024 * 1024 ? `${(n / (1024 * 1024)).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`

  const phaseText = phaseMap[phase] ?? 'Working…'

  const installPhases: ReadonlyArray<UpdatePhase> = [
    'mounting',
    'copying',
    'unmounting',
    'installing',
    'relaunching'
  ]

  const dialogTitle = isDowngrade ? UpgradeText.downgrade : UpgradeText.upgrade

  const closeAndReset = useCallback(() => {
    setUpDialogOpen(false)
    setInFlight(false)
    setPercent(null)
    setReceived(0)
    setTotal(0)
    setError('')
    setPhase('start')
  }, [])

  const recheckLatest = useCallback(async () => {
    try {
      setMessage('')
      const r = await window.app?.getLatestRelease?.()
      if (r?.version) setLatestVersion(r.version)
      else setLatestVersion('—')
      setLatestUrl(r?.url)
      if (!r?.version) setMessage('Could not check latest release.')
    } catch (err) {
      console.warn('[SoftwareUpdate] getLatestRelease failed', err)
      setLatestVersion('—')
      setLatestUrl(undefined)
      setMessage('Could not check latest release.')
    }
  }, [])

  useEffect(() => {
    window.app?.getVersion?.().then((v) => v && setInstalledVersion(v))
    recheckLatest()
  }, [recheckLatest])

  useEffect(() => {
    if (phase === 'ready' && !upDialogOpen) setUpDialogOpen(true)
  }, [phase, upDialogOpen])

  useEffect(() => {
    if (phase === 'error' && /aborted/i.test(error || '')) {
      const t = setTimeout(closeAndReset, 1200)
      return () => clearTimeout(t)
    }
    return
  }, [phase, error, closeAndReset])

  useEffect(() => {
    const off1 = window.app?.onUpdateEvent?.((e: UpdateEvent) => {
      setPhase(e.phase as UpdatePhase)
      setInFlight(e.phase !== 'error' && e.phase !== 'start')
      if (e.phase === 'error') {
        setError(e.message ?? 'Update failed')
        setMessage(e.message ?? 'Update failed')
      } else {
        setError('')
      }
    })

    const off2 = window.app?.onUpdateProgress?.((p: UpdateProgress) => {
      setInFlight(true)
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

  const actionEnabled = !hasLatest ? true : cmp !== null && cmp !== 0 && !inFlight

  const triggerUpdate = useCallback(() => {
    setMessage('')
    setError('')
    setPercent(null)
    setReceived(0)
    setTotal(0)
    setPhase('start')
    setUpDialogOpen(true)
    setInFlight(true)
    window.app?.performUpdate?.(latestUrl)
  }, [latestUrl])

  const onPrimaryAction = useCallback(() => {
    if (!hasLatest) {
      recheckLatest()
      return
    }
    if (inFlight) {
      setUpDialogOpen(true)
      return
    }
    if (cmp !== 0) triggerUpdate()
  }, [hasLatest, inFlight, cmp, triggerUpdate, recheckLatest])

  const primaryLabel = useMemo(() => {
    if (!hasLatest) return 'Check'
    if (cmp == null) return 'Update'
    if (cmp < 0) return 'Update'
    if (cmp > 0) return 'Downgrade'
    return 'Up to date'
  }, [hasLatest, cmp])

  const availabilityStatus = useMemo(() => {
    if (!hasLatest || cmp == null) return '—'
    if (cmp < 0) return 'Update available'
    if (cmp > 0) return 'Downgrade available'
    return 'Up to date'
  }, [hasLatest, cmp])

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Stack spacing={0.75}>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            Installed:
          </Typography>
          <Typography sx={{ fontVariantNumeric: 'tabular-nums' }}>{installedVersion}</Typography>
        </Stack>

        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            Available:
          </Typography>
          <Typography sx={{ fontVariantNumeric: 'tabular-nums' }}>{latestVersion}</Typography>
        </Stack>

        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            Status:
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {availabilityStatus}
          </Typography>
        </Stack>
      </Stack>

      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
        <Button variant="contained" onClick={onPrimaryAction} disabled={!actionEnabled}>
          {primaryLabel}
        </Button>

        {(inFlight || phase === 'download') && <CircularProgress size={18} />}

        <Button variant="outlined" onClick={recheckLatest} disabled={inFlight}>
          Refresh
        </Button>
      </Stack>

      {message && (
        <Typography variant="body2" color={error ? 'error' : 'text.secondary'}>
          {message}
        </Typography>
      )}

      <Dialog open={upDialogOpen} onClose={() => {}} disableEscapeKeyDown>
        <DialogTitle>{dialogTitle}</DialogTitle>
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

          {installPhases.includes(phase) && (
            <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
              Restarts automatically when done.
            </Typography>
          )}
        </DialogContent>

        <DialogActions>
          <Button
            onClick={() => {
              window.app?.abortUpdate?.()
            }}
            disabled={!(phase === 'download' ? pct == null || pct < 100 : phase === 'ready')}
          >
            Abort
          </Button>

          {phase === 'ready' && (
            <Button variant="contained" onClick={() => window.app?.beginInstall?.()}>
              Install now
            </Button>
          )}

          {phase === 'error' && (
            <Button
              variant="outlined"
              onClick={() => {
                closeAndReset()
              }}
            >
              Close
            </Button>
          )}
        </DialogActions>
      </Dialog>
    </Box>
  )
}
