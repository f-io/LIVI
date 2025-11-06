import { ExtraConfig } from '@main/Globals'
import React, { useEffect, useMemo, useState, startTransition, useCallback } from 'react'
import {
  Box,
  FormControlLabel,
  TextField,
  Checkbox,
  FormControl,
  FormLabel,
  Button,
  Dialog,
  DialogTitle,
  DialogContent,
  Slide,
  Stack,
  Grid,
  Slider,
  CircularProgress,
  Typography,
  MenuItem,
  InputAdornment
} from '@mui/material'
import { useTheme } from '@mui/material/styles'
import type { SxProps, Theme } from '@mui/material/styles'
import { TransitionProps } from '@mui/material/transitions'
import { KeyBindings } from '../../keyBindings'
import { updateCameras as detectCameras } from '@utils/cameraDetection'
import debounce from 'lodash.debounce'
import { useCarplayStore, useStatusStore } from '@store/store'

interface SettingsProps {
  settings: ExtraConfig | null
}

const MEDIA_DELAY_MIN = 300
const MEDIA_DELAY_MAX = 2000
const HEIGHT_MIN = 200
const MIN_WIDTH = 400

const UI_DEBOUNCED_KEYS = new Set<keyof ExtraConfig>(['primaryColorDark', 'primaryColorLight'])

type UsbEvent = { type?: string } & Record<string, unknown>

const Transition = React.forwardRef(function Transition(
  props: TransitionProps & { children: React.ReactElement },
  ref: React.Ref<unknown>
) {
  return <Slide direction="up" ref={ref} {...props} />
})

const SectionHeader: React.FC<{ children: React.ReactNode; sx?: SxProps<Theme> }> = ({
  children,
  sx
}) => {
  const theme = useTheme()
  return (
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
        },
        ...sx
      }}
    >
      {children}
    </Typography>
  )
}

function coerceSelectValue<T extends string | number>(
  value: T | null | undefined,
  options: readonly T[]
): T | '' {
  return value != null && options.includes(value as T) ? (value as T) : ''
}

export const Settings: React.FC<SettingsProps> = ({ settings }) => {
  const hasSettings = !!settings

  const [activeSettings, setActiveSettings] = useState<ExtraConfig>(() => {
    const base = (settings ?? ({} as ExtraConfig)) as ExtraConfig
    return {
      ...base,
      audioVolume: base?.audioVolume ?? 1.0,
      navVolume: base?.navVolume ?? 1.0
    }
  })
  useEffect(() => {
    if (!settings) return
    setActiveSettings({
      ...settings,
      audioVolume: settings.audioVolume ?? 1.0,
      navVolume: settings.navVolume ?? 1.0
    })
  }, [settings])

  const [micLabel, setMicLabel] = useState('not available')
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([])
  const [openBindings, setOpenBindings] = useState(false)
  const [isResetting, setIsResetting] = useState(false)
  const [resetMessage, setResetMessage] = useState('')
  const [closeCountdown, setCloseCountdown] = useState(0)
  const [hasChanges, setHasChanges] = useState(false)
  const [openAdvanced, setOpenAdvanced] = useState(false)
  const [micResetPending, setMicResetPending] = useState(false)

  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)
  const theme = useTheme()
  const isDarkMode = theme.palette.mode === 'dark'
  const currentPrimary =
    (isDarkMode ? activeSettings.primaryColorDark : activeSettings.primaryColorLight) ??
    theme.palette.primary.main

  const debouncedSave = useMemo(
    () => debounce((newSettings: ExtraConfig) => saveSettings(newSettings), 500),
    [saveSettings]
  )
  useEffect(() => () => debouncedSave.cancel(), [debouncedSave])

  const autoSave = useCallback(
    async (patch: Partial<ExtraConfig>) => {
      let kiosk = false
      try {
        kiosk = await window.app.getKiosk()
      } catch {}
      setActiveSettings((prev) => {
        const storeSettings = useCarplayStore.getState().settings
        const nightMode =
          typeof storeSettings?.nightMode === 'boolean' ? storeSettings.nightMode : prev.nightMode
        const updated: ExtraConfig = { ...prev, ...patch, kiosk, nightMode }
        saveSettings(updated)
        return updated
      })
    },
    [saveSettings]
  )

  const requiresRestartParams: (keyof ExtraConfig)[] = [
    'width',
    'height',
    'fps',
    'dpi',
    'format',
    'mediaDelay',
    'wifiType',
    'audioTransferMode'
  ]

  const getValidWifiChannel = (wifiType: ExtraConfig['wifiType'], ch?: number): number => {
    if (wifiType === '5ghz') {
      return typeof ch === 'number' && ch >= 36 ? ch : 36
    }
    return typeof ch === 'number' && ch > 0 && ch < 36 ? ch : 6
  }

  const settingsChange = <K extends SettingKey>(key: K, value: ExtraConfig[K]) => {
    if (key === 'micType') {
      const prev = activeSettings.micType
      const next: ExtraConfig['micType'] = value === 'box' || value === 'os' ? value : prev
      const updated = withSetting(activeSettings, 'micType', next)
      startTransition(() => setActiveSettings(updated))
      saveSettings(updated)
      setMicResetPending(prev !== 'box' && next === 'box' && isDongleConnected)
      return
    }

    const guardedValue = sanitizeSetting(key, value, activeSettings)
    let updated = withSetting(activeSettings, key, guardedValue)

    if (key === 'wifiType') {
      updated = {
        ...updated,
        wifiChannel: getValidWifiChannel(
          guardedValue as ExtraConfig['wifiType'],
          updated.wifiChannel
        )
      }
    }

    startTransition(() => setActiveSettings(updated))

    if (
      key === 'audioVolume' ||
      key === 'navVolume' ||
      key === 'bindings' ||
      key === 'camera' ||
      UI_DEBOUNCED_KEYS.has(key)
    ) {
      debouncedSave(updated)
    } else if (key === 'kiosk' || key === 'nightMode') {
      saveSettings(updated)
    } else if (requiresRestartParams.includes(key)) {
      if (hasSettings) {
        const prev = settings!
        const pending = requiresRestartParams.some((param) => updated[param] !== prev[param])
        setHasChanges(pending)
      } else {
        setHasChanges(false)
      }
    }
  }

  const handleSave = async () => {
    const needsReset = hasChanges || micResetPending

    if (needsReset) {
      setIsResetting(true)
      setCloseCountdown(3)
    }

    let resetStatus = ''
    try {
      if (needsReset && isDongleConnected) {
        setResetMessage('Dongle Reset...')
        await window.carplay.usb.forceReset()
        resetStatus = 'Success'
      } else {
        resetStatus = 'Settings saved (no dongle connected)'
      }
    } catch {
      resetStatus = 'Dongle Reset Error.'
    }

    await saveSettings(activeSettings)
    setHasChanges(false)
    setMicResetPending(false)
    setIsResetting(false)
    setResetMessage(resetStatus)
  }

  type SettingKey = keyof ExtraConfig

  function sanitizeSetting<K extends SettingKey>(
    key: K,
    raw: unknown,
    current: ExtraConfig
  ): ExtraConfig[K] {
    switch (key) {
      case 'mediaDelay': {
        const n = Number(raw)
        const v = Number.isFinite(n)
          ? Math.round(Math.min(MEDIA_DELAY_MAX, Math.max(MEDIA_DELAY_MIN, n)))
          : current.mediaDelay
        return v as ExtraConfig[K]
      }
      case 'height': {
        const n = Number(raw)
        const v = Number.isFinite(n) ? Math.round(Math.max(HEIGHT_MIN, n)) : current.height
        return v as ExtraConfig[K]
      }
      case 'width': {
        const n = Number(raw)
        const v = Number.isFinite(n) ? Math.round(Math.max(MIN_WIDTH, n)) : current.width
        return v as ExtraConfig[K]
      }

      case 'fps':
      case 'dpi':
      case 'format':
      case 'iBoxVersion':
      case 'phoneWorkMode':
      case 'packetMax':
      case 'wifiChannel': {
        const n = Number(raw)
        return Number.isFinite(n) ? (n as unknown as ExtraConfig[K]) : current[key]
      }

      case 'audioVolume':
      case 'navVolume': {
        const n = Number(raw)
        const v = Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : (current[key] as number)
        return v as unknown as ExtraConfig[K]
      }

      case 'kiosk':
      case 'nightMode':
      case 'audioTransferMode':
        return Boolean(raw) as ExtraConfig[K]

      case 'wifiType': {
        const v = raw === '2.4ghz' ? '2.4ghz' : raw === '5ghz' ? '5ghz' : current.wifiType
        return v as ExtraConfig[K]
      }
      case 'micType': {
        const v = raw === 'box' ? 'box' : raw === 'os' ? 'os' : current.micType
        return v as ExtraConfig[K]
      }

      case 'primaryColorDark':
      case 'primaryColorLight':
      case 'camera':
      case 'microphone':
      case 'boxName':
        return (raw === undefined ? undefined : String(raw)) as ExtraConfig[K]

      case 'bindings':
        return raw as ExtraConfig[K]

      default:
        return raw as ExtraConfig[K]
    }
  }

  function withSetting<K extends SettingKey>(
    base: ExtraConfig,
    key: K,
    val: ExtraConfig[K]
  ): ExtraConfig {
    return { ...base, [key]: val }
  }

  const toWifiType = (s: string): ExtraConfig['wifiType'] => (s === '5ghz' ? '5ghz' : '2.4ghz')

  useEffect(() => {
    if (!resetMessage) return
    const timerId = setInterval(() => {
      setCloseCountdown((prev) => {
        if (prev <= 1) {
          clearInterval(timerId)
          setResetMessage('')
          return 0
        }
        return prev - 1
      })
    }, 1000)
    return () => clearInterval(timerId)
  }, [resetMessage])

  const renderOsMicLabel = (raw: string) => {
    if (raw === 'not available') return 'No system input'
    const lower = raw.toLowerCase()
    if (lower === 'system default' || lower === 'default') return 'OS mic (auto)'
    return `OS • ${raw}`
  }

  const updateMic = useCallback(async () => {
    try {
      const label = await window.carplay.usb.getSysdefaultPrettyName()
      const final = label && !['sysdefault', 'null'].includes(label) ? label : 'not available'
      setMicLabel(final)

      if (!activeSettings.micType) {
        await autoSave({ micType: (final === 'not available' ? 'box' : 'os') as 'box' | 'os' })
      }
    } catch {
      console.warn('[Settings] Mic label fetch failed')
    }
  }, [activeSettings.micType, autoSave])

  useEffect(() => {
    updateMic()
    const micUsbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as UsbEvent
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        updateMic()
      }
    }
    window.carplay.usb.listenForEvents(micUsbHandler)
    return () => window.carplay.usb.unlistenForEvents(micUsbHandler)
  }, [updateMic])

  const safeCameraPersist = useCallback(
    async (cfgOrId: string | { camera?: string } | null | undefined) => {
      if (activeSettings.camera && activeSettings.camera !== '') return
      const cameraId = typeof cfgOrId === 'string' ? cfgOrId : cfgOrId?.camera
      await autoSave({ camera: cameraId ?? '' })
    },
    [autoSave, activeSettings.camera]
  )

  useEffect(() => {
    detectCameras(setCameraFound, safeCameraPersist, activeSettings).then(setCameras)

    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as UsbEvent
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        detectCameras(setCameraFound, safeCameraPersist, activeSettings).then(setCameras)
      }
    }
    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents(usbHandler)
  }, [activeSettings, safeCameraPersist, setCameraFound])

  useEffect(() => {
    let off: (() => void) | undefined
    ;(async () => {
      try {
        const kiosk = await window.app.getKiosk()
        startTransition(() =>
          setActiveSettings((prev) => (prev.kiosk === kiosk ? prev : { ...prev, kiosk }))
        )
      } catch {
        // ignore
      }

      off = window.app.onKioskSync((kiosk) => {
        startTransition(() =>
          setActiveSettings((prev) => (prev.kiosk === kiosk ? prev : { ...prev, kiosk }))
        )
      })
    })()

    return () => {
      if (off) off()
    }
  }, [])

  const handleClosePopup = () => {
    setResetMessage('')
    setCloseCountdown(0)
  }

  const micUnavailable = micLabel === 'not available'

  const cameraOptions = useMemo<readonly { deviceId: string; label: string }[]>(
    () =>
      cameras.length
        ? cameras.map((c) => ({ deviceId: c.deviceId ?? '', label: c.label || 'Camera' }))
        : [{ deviceId: '', label: 'No camera' }],
    [cameras]
  )
  const cameraIds = useMemo<readonly string[]>(
    () => cameraOptions.map((c) => c.deviceId),
    [cameraOptions]
  )
  const cameraValue = coerceSelectValue(activeSettings.camera ?? '', cameraIds)

  const wifiOptions = ['2.4ghz', '5ghz'] as const
  const wifiValue = coerceSelectValue(
    (activeSettings.wifiType as unknown as string) ?? '',
    wifiOptions as unknown as string[]
  )

  if (!hasSettings) return null

  return (
    <Box
      id="settings-root"
      className={theme.palette.mode === 'dark' ? 'App-header-dark' : 'App-header-light'}
      p={2}
      display="flex"
      flexDirection="column"
      height="100vh"
    >
      <Box
        sx={{
          overflowY: 'hidden',
          overflowX: 'hidden',
          flexGrow: 1,
          px: 1.5,
          py: 1.5,
          display: 'flex',
          flexDirection: 'column',
          gap: 2
        }}
      >
        <Grid container spacing={2} sx={{ px: 1 }}>
          <Grid size={{ xs: 12, sm: 6 }}>
            <SectionHeader sx={{ mb: 2.25 }}>VIDEO SETTINGS</SectionHeader>

            <Box sx={{ pl: 1.5 }}>
              <Box
                sx={{
                  display: 'grid',
                  gridTemplateColumns: '136px 24px 136px',
                  alignItems: 'center',
                  gap: 2,
                  width: 'fit-content'
                }}
              >
                <TextField
                  size="small"
                  label="WIDTH"
                  type="number"
                  value={activeSettings.width}
                  onChange={(e) => settingsChange('width', Number(e.target.value))}
                  InputProps={{
                    inputProps: { min: MIN_WIDTH, step: 1 },
                    endAdornment: <InputAdornment position="end">px</InputAdornment>
                  }}
                  sx={{ width: 136 }}
                />
                <Typography sx={{ textAlign: 'center', fontSize: 22, lineHeight: 1 }}>×</Typography>
                <TextField
                  size="small"
                  label="HEIGHT"
                  type="number"
                  value={activeSettings.height}
                  onChange={(e) => settingsChange('height', Number(e.target.value))}
                  InputProps={{
                    inputProps: { min: HEIGHT_MIN, step: 1 },
                    endAdornment: <InputAdornment position="end">px</InputAdornment>
                  }}
                  sx={{ width: 136 }}
                />
              </Box>

              <Box
                sx={{
                  mt: 1.75,
                  display: 'grid',
                  gridTemplateColumns: '136px 24px 136px',
                  alignItems: 'center',
                  gap: 2,
                  width: 'fit-content'
                }}
              >
                <TextField
                  size="small"
                  label="FPS"
                  type="number"
                  value={activeSettings.fps}
                  onChange={(e) => settingsChange('fps', Number(e.target.value))}
                  sx={{ width: 136 }}
                />
                <Box sx={{ width: 24, height: 1 }} />
                <TextField
                  size="small"
                  label="MEDIA DELAY"
                  type="number"
                  value={activeSettings.mediaDelay}
                  onChange={(e) => settingsChange('mediaDelay', Number(e.target.value))}
                  InputProps={{
                    inputProps: { min: MEDIA_DELAY_MIN, max: MEDIA_DELAY_MAX, step: 50 },
                    endAdornment: <InputAdornment position="end">ms</InputAdornment>
                  }}
                  sx={{ width: 136 }}
                />
              </Box>
            </Box>
          </Grid>

          <Grid size={{ xs: 12, sm: 6 }}>
            <SectionHeader>AUDIO SETTINGS</SectionHeader>

            <Stack spacing={1.5} sx={{ pl: 1.5, maxWidth: 360 }}>
              <FormControl fullWidth>
                <FormLabel sx={{ typography: 'body2' }}>AUDIO VOLUME</FormLabel>
                <Slider
                  size="small"
                  value={Math.round((activeSettings.audioVolume ?? 1.0) * 100)}
                  min={0}
                  max={100}
                  step={5}
                  marks
                  valueLabelDisplay="auto"
                  onChange={(_, v) =>
                    typeof v === 'number' && settingsChange('audioVolume', v / 100)
                  }
                />
              </FormControl>

              <FormControl fullWidth>
                <FormLabel sx={{ typography: 'body2' }}>NAV VOLUME</FormLabel>
                <Slider
                  size="small"
                  value={Math.round((activeSettings.navVolume ?? 1.0) * 100)}
                  min={0}
                  max={100}
                  step={5}
                  marks
                  valueLabelDisplay="auto"
                  onChange={(_, v) => typeof v === 'number' && settingsChange('navVolume', v / 100)}
                />
              </FormControl>
            </Stack>
          </Grid>
        </Grid>

        <Grid container spacing={2} sx={{ px: 1 }} columns={12} alignItems="center">
          <Grid size={{ xs: 6, sm: 3 }}>
            <Stack spacing={0.5}>
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.kiosk}
                    onChange={(e) => settingsChange('kiosk', e.target.checked)}
                  />
                }
                label="KIOSK"
              />
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.nightMode}
                    onChange={(e) => settingsChange('nightMode', e.target.checked)}
                  />
                }
                label="DARK MODE"
              />
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.audioTransferMode}
                    onChange={(e) => settingsChange('audioTransferMode', e.target.checked)}
                  />
                }
                label="DISABLE AUDIO"
              />
            </Stack>
          </Grid>

          <Grid size={{ xs: 6, sm: 3 }} sx={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              select
              fullWidth
              label="WIFI"
              value={wifiValue}
              onChange={(e) => settingsChange('wifiType', toWifiType(e.target.value))}
            >
              <MenuItem value="2.4ghz">2.4 GHz</MenuItem>
              <MenuItem value="5ghz">5 GHz</MenuItem>
            </TextField>
          </Grid>

          <Grid size={{ xs: 6, sm: 3 }} sx={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              select
              fullWidth
              label="MICROPHONE"
              value={activeSettings.micType}
              onChange={(e) => settingsChange('micType', e.target.value as 'box' | 'os')}
            >
              <MenuItem value="os" disabled={micUnavailable && activeSettings.micType !== 'os'}>
                <Typography noWrap component="span" title={micLabel}>
                  {renderOsMicLabel(micLabel)}
                </Typography>
              </MenuItem>
              <MenuItem value="box">BOX</MenuItem>
            </TextField>
          </Grid>

          <Grid size={{ xs: 6, sm: 3 }} sx={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              select
              fullWidth
              label="CAMERA"
              value={cameraValue}
              onChange={(e) => settingsChange('camera', e.target.value)}
            >
              {cameraOptions.map((cam) => (
                <MenuItem key={cam.deviceId || 'none'} value={cam.deviceId}>
                  {cam.label}
                </MenuItem>
              ))}
            </TextField>
          </Grid>
        </Grid>
      </Box>

      <Box
        position="sticky"
        bottom={0}
        bgcolor="transparent"
        display="flex"
        justifyContent="center"
        sx={{ pt: 1, pb: 1 }}
      >
        <Box sx={{ display: 'flex', gap: 1.5 }}>
          <Button variant="outlined" onClick={() => setOpenAdvanced(true)}>
            ADVANCED
          </Button>
          <Button variant="outlined" onClick={() => setOpenBindings(true)}>
            BINDINGS
          </Button>
          <Button
            variant="contained"
            className="hover-ring"
            color={hasChanges || micResetPending ? 'primary' : 'inherit'}
            onClick={hasChanges || micResetPending ? handleSave : undefined}
            disabled={!(hasChanges || micResetPending) || isResetting}
          >
            SAVE
          </Button>
        </Box>
      </Box>

      {isResetting && (
        <Box display="flex" justifyContent="center" sx={{ mt: 1.5 }}>
          <CircularProgress />
        </Box>
      )}

      <Dialog open={!!resetMessage} onClose={handleClosePopup}>
        <DialogTitle>Reset Status</DialogTitle>
        <DialogContent sx={{ textAlign: 'center' }}>
          <Typography variant="body1" sx={{ mb: 2 }}>
            {resetMessage}
          </Typography>
          <Box display="flex" justifyContent="center">
            <Button variant="outlined" onClick={handleClosePopup}>
              Close{closeCountdown > 0 ? ` (${closeCountdown})` : ''}
            </Button>
          </Box>
        </DialogContent>
      </Dialog>

      <Dialog
        open={openBindings}
        TransitionComponent={Transition}
        keepMounted
        PaperProps={{ sx: { minHeight: '80%', minWidth: '80%' } }}
        onClose={() => setOpenBindings(false)}
      >
        <DialogTitle>Key Bindings</DialogTitle>
        <DialogContent>
          <KeyBindings settings={activeSettings} updateKey={settingsChange} />
        </DialogContent>
      </Dialog>

      <Dialog
        open={openAdvanced}
        TransitionComponent={Transition}
        keepMounted
        onClose={() => setOpenAdvanced(false)}
        maxWidth={false}
        PaperProps={{
          sx: {
            width: 320,
            maxWidth: 'calc(100vw - 48px)',
            borderRadius: 2
          }
        }}
      >
        <DialogTitle>Advanced Settings</DialogTitle>
        <DialogContent sx={{ pt: 2, pb: 2, px: 2.25, overflow: 'visible' }}>
          <Box
            sx={(theme) => ({
              display: 'grid',
              gridTemplateColumns: '120px 120px',
              columnGap: theme.spacing(1.5),
              rowGap: theme.spacing(1.5),
              justifyContent: 'center'
            })}
          >
            <TextField
              size="small"
              label={isDarkMode ? 'PRIMARY (DARK)' : 'PRIMARY (LIGHT)'}
              value={currentPrimary}
              onChange={(e) =>
                settingsChange(
                  isDarkMode ? 'primaryColorDark' : 'primaryColorLight',
                  e.target.value
                )
              }
              InputProps={{
                inputProps: { type: 'color' },
                endAdornment: (
                  <InputAdornment position="end">
                    <Button
                      size="small"
                      variant="outlined"
                      onClick={() =>
                        settingsChange(
                          isDarkMode ? 'primaryColorDark' : 'primaryColorLight',
                          undefined
                        )
                      }
                      sx={{ ml: 1, py: 0.25, px: 1 }}
                    >
                      RESET
                    </Button>
                  </InputAdornment>
                )
              }}
              InputLabelProps={{ shrink: true }}
              fullWidth
              sx={{ gridColumn: '1 / span 2' }}
            />
            <TextField
              label="DPI"
              type="number"
              size="small"
              margin="dense"
              value={activeSettings.dpi}
              onChange={(e) => settingsChange('dpi', Number(e.target.value))}
              InputLabelProps={{ shrink: true }}
              autoFocus={openAdvanced}
              inputProps={{ tabIndex: openAdvanced ? 0 : -1 }}
              sx={{ width: 120 }}
            />
            <TextField
              label="FORMAT"
              type="number"
              size="small"
              margin="dense"
              value={activeSettings.format}
              onChange={(e) => settingsChange('format', Number(e.target.value))}
              InputLabelProps={{ shrink: true }}
              sx={{ width: 120 }}
            />
          </Box>
        </DialogContent>
      </Dialog>
    </Box>
  )
}
