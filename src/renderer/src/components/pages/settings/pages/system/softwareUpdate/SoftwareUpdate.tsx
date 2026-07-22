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
  Switch,
  Typography
} from '@mui/material'
import { EMPTY_STRING } from '@renderer/constants'
import type { Config } from '@shared/types'
import { useLiviStore } from '@store/store'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CMP_CONFIG, INSTALL_PHASES } from './constants'
import { phaseMap, UpdatePhases, UpgradeText } from './types'
import { buildTag, cmpSemver, human, parseSemver, sameNightlyBuild } from './utils'

export function SoftwareUpdate() {
  const { t } = useTranslation()
  const [installedVersion, setInstalledVersion] = useState<string>(EMPTY_STRING)
  const [latestVersion, setLatestVersion] = useState<string>(EMPTY_STRING)
  const [latestUrl, setLatestUrl] = useState<string | undefined>(undefined)
  const [latestCommit, setLatestCommit] = useState<string>(EMPTY_STRING)
  const [latestRun, setLatestRun] = useState<string>(EMPTY_STRING)

  const settings = useLiviStore((s) => s.settings) as Config | null
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const isNightly = settings?.updateNightly === true
  const installedSha = typeof __BUILD_SHA__ === 'string' ? __BUILD_SHA__ : EMPTY_STRING
  const installedRun = typeof __BUILD_RUN__ === 'string' ? __BUILD_RUN__ : EMPTY_STRING

  const [message, setMessage] = useState<string>('')

  const [upDialogOpen, setUpDialogOpen] = useState(false)
  const [phase, setPhase] = useState<UpdatePhases>(UpdatePhases.start)
  const [percent, setPercent] = useState<number | null>(null)
  const [received, setReceived] = useState<number>(0)
  const [total, setTotal] = useState<number>(0)
  const [error, setError] = useState<string>('')

  const [inFlight, setInFlight] = useState(false)
  const [installStarting, setInstallStarting] = useState(false)

  const installedSem = parseSemver(installedVersion)
  const latestSem = parseSemver(latestVersion)

  const hasLatest = isNightly ? Boolean(latestUrl) : Boolean(latestUrl && latestSem && installedSem)
  const cmp = !hasLatest
    ? null
    : isNightly
      ? sameNightlyBuild(installedSha, latestCommit)
        ? 0
        : -1
      : cmpSemver(installedSem!, latestSem!)
  const isDowngrade = cmp != null && cmp > 0
  const pct = percent != null ? Math.round(percent * 100) : null
  const phaseText = phaseMap[phase] ?? 'Working…'
  const dialogTitle = isDowngrade ? UpgradeText.downgrade : UpgradeText.upgrade

  const resetUpdateState = useCallback(() => {
    setPercent(null)
    setReceived(0)
    setTotal(0)
    setError('')
    setPhase(UpdatePhases.start)
    setInFlight(false)
    setInstallStarting(false)
  }, [])

  const handleCloseAndReset = useCallback(() => {
    setUpDialogOpen(false)
    resetUpdateState()
  }, [resetUpdateState])

  const handleRecheckLatest = useCallback(async () => {
    try {
      setMessage('')
      const r = await window.app?.getLatestRelease?.()
      if (r?.version) setLatestVersion(r.version)
      else setLatestVersion(EMPTY_STRING)
      setLatestUrl(r?.url)
      setLatestCommit(r?.commit ?? EMPTY_STRING)
      setLatestRun(r?.run ?? EMPTY_STRING)
      if (!r?.url) setMessage(t('softwareUpdate.couldNotCheckLatestRelease'))
    } catch (err) {
      console.warn('[SoftwareUpdate] getLatestRelease failed', err)
      setLatestVersion(EMPTY_STRING)
      setLatestUrl(undefined)
      setLatestCommit(EMPTY_STRING)
      setLatestRun(EMPTY_STRING)
      setMessage(t('softwareUpdate.couldNotCheckLatestRelease'))
    }
  }, [t])

  const handleNightlyChange = useCallback(
    (_e: unknown, nightly: boolean) => {
      if (!settings || nightly === isNightly) return
      saveSettings({ ...settings, updateNightly: nightly })
    },
    [isNightly, saveSettings, settings]
  )

  useEffect(() => {
    window.app?.getVersion?.().then((v) => v && setInstalledVersion(v))
  }, [])

  useEffect(() => {
    handleRecheckLatest()
  }, [handleRecheckLatest, isNightly])

  useEffect(() => {
    if (phase === UpdatePhases.ready && !upDialogOpen) setUpDialogOpen(true)
  }, [phase, upDialogOpen])

  useEffect(() => {
    if (phase === UpdatePhases.error && /aborted/i.test(error || '')) {
      const t = setTimeout(handleCloseAndReset, 1200)
      return () => clearTimeout(t)
    }
    return
  }, [phase, error, handleCloseAndReset])

  useEffect(() => {
    const off1 = window.app?.onUpdateEvent?.((e: UpdateEvent) => {
      setPhase(e.phase as UpdatePhases)
      setInFlight(e.phase !== UpdatePhases.error && e.phase !== UpdatePhases.start)
      if (e.phase === UpdatePhases.error) {
        setError(e.message ?? t('softwareUpdate.updateFailed'))
        setMessage(e.message ?? t('softwareUpdate.updateFailed'))
      } else {
        setError('')
      }
    })

    const off2 = window.app?.onUpdateProgress?.((p: UpdateProgress) => {
      setInFlight(true)
      setPhase(UpdatePhases.download)
      setPercent(typeof p.percent === 'number' ? Math.max(0, Math.min(1, p.percent)) : null)
      setReceived(p.received ?? 0)
      setTotal(p.total ?? 0)
    })

    return () => {
      off1?.()
      off2?.()
    }
  }, [t])

  const canUpdate = cmp != null && cmp !== 0 && !inFlight
  const actionEnabled = !hasLatest || canUpdate

  const triggerUpdate = useCallback(() => {
    setMessage('')
    setUpDialogOpen(true)
    resetUpdateState()
    window.app?.performUpdate?.(latestUrl)
  }, [latestUrl, resetUpdateState])

  const handlePrimaryAction = useCallback(() => {
    if (!hasLatest) {
      handleRecheckLatest()
      return
    }
    if (inFlight) {
      setUpDialogOpen(true)
      return
    }
    if (cmp !== 0) triggerUpdate()
  }, [hasLatest, inFlight, cmp, triggerUpdate, handleRecheckLatest])

  const versionInfo = useMemo(() => {
    if (!hasLatest || cmp == null) {
      return { label: t('softwareUpdate.check'), status: EMPTY_STRING }
    }

    return CMP_CONFIG[cmp] ?? { label: t('softwareUpdate.update'), status: EMPTY_STRING }
  }, [hasLatest, cmp, t])

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
      <Stack spacing={0.75}>
        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            {t('softwareUpdate.installedVersion')}:
          </Typography>
          <Typography sx={{ fontVariantNumeric: 'tabular-nums' }}>
            {installedVersion}
            {isNightly ? buildTag(installedRun, installedSha) : ''}
          </Typography>
        </Stack>

        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            {t('softwareUpdate.availableVersion')}:
          </Typography>
          <Typography sx={{ fontVariantNumeric: 'tabular-nums' }}>
            {latestVersion}
            {isNightly ? buildTag(latestRun, latestCommit.slice(0, 7)) : ''}
          </Typography>
        </Stack>

        <Stack direction="row" spacing={1} sx={{ alignItems: 'baseline' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            {t('softwareUpdate.status')}:
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {versionInfo.status}
          </Typography>
        </Stack>

        <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
          <Typography sx={{ minWidth: 96 }} color="text.secondary">
            {t('softwareUpdate.channelNightly')}:
          </Typography>
          <Switch
            checked={isNightly}
            disabled={inFlight}
            onChange={handleNightlyChange}
            slotProps={{ input: { 'aria-label': t('softwareUpdate.channelNightly') } }}
          />
        </Stack>
      </Stack>

      <Stack direction="row" spacing={1} sx={{ alignItems: 'center', flexWrap: 'wrap' }}>
        <Button variant="contained" onClick={handlePrimaryAction} disabled={!actionEnabled}>
          {versionInfo.label}
        </Button>

        {(inFlight || phase === UpdatePhases.download) && <CircularProgress size={18} />}

        <Button variant="outlined" onClick={handleRecheckLatest} disabled={inFlight}>
          {t('softwareUpdate.refresh')}
        </Button>
      </Stack>

      {message && (
        <Typography variant="body2" color={error ? 'error' : 'text.secondary'}>
          {message}
        </Typography>
      )}

      <Dialog
        open={upDialogOpen}
        onClose={(event, reason) => {
          if (phase !== UpdatePhases.error && reason === 'escapeKeyDown') return
          handleCloseAndReset()
        }}
      >
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

          {INSTALL_PHASES.includes(phase) && (
            <Typography variant="body2" sx={{ mt: 1 }} color="text.secondary">
              {t('softwareUpdate.restartsAutomaticallyWhenDone')}
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
            {t('softwareUpdate.abort')}
          </Button>

          {phase === 'ready' && (
            <Button
              variant="contained"
              disabled={installStarting}
              onClick={() => {
                if (installStarting) return
                setInstallStarting(true)
                window.app?.beginInstall?.()
              }}
            >
              {t('softwareUpdate.installNow')}
            </Button>
          )}

          {phase === 'error' && (
            <Button
              variant="outlined"
              onClick={() => {
                handleCloseAndReset()
              }}
            >
              {t('softwareUpdate.close')}
            </Button>
          )}
        </DialogActions>
      </Dialog>
    </Box>
  )
}
