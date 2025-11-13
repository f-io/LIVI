import { ExtraConfig } from '@main/Globals'
import React, {
  useEffect,
  useMemo,
  useState,
  startTransition,
  useCallback,
  useContext
} from 'react'
import {
  Box,
  Divider,
  FormControlLabel,
  TextField,
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
  Tooltip,
  Typography,
  MenuItem,
  InputAdornment,
  Switch,
  Paper
} from '@mui/material'
import {
  DarkModeOutlined,
  LightModeOutlined,
  VolumeOffOutlined,
  VolumeUpOutlined,
  PlayCircleOutline,
  FullscreenOutlined
} from '@mui/icons-material'
import { useTheme } from '@mui/material/styles'
import type { SxProps, Theme } from '@mui/material/styles'
import { TransitionProps } from '@mui/material/transitions'
import { KeyBindings } from '../../keyBindings'
import { updateCameras as detectCameras } from '@utils/cameraDetection'
import debounce from 'lodash.debounce'
import type { DebouncedFunc } from 'lodash'
import { useCarplayStore, useStatusStore } from '@store/store'
import {
  CAR_NAME_MAX,
  DEFAULT_FPS,
  DEFAULT_HEIGHT,
  DEFAULT_WIDTH,
  MAX_HEIGHT,
  MAX_WIDTH,
  MAX_FPS,
  MIN_FPS,
  MIN_HEIGHT,
  MIN_WIDTH,
  MEDIA_DELAY_MIN,
  MEDIA_DELAY_MAX,
  OEM_LABEL_MAX,
  UI_DEBOUNCED_KEYS,
  WiFiValues,
  requiresRestartParams
} from './constants'
import { AppContext } from '../../../context'
import { themeColors } from '../../../themeColors'
import { THEME } from '../../../constants'
import { highlightEditableField } from './utils'

function normalizeCarName(input: string): string {
  const ascii = input.replace(/[^\x20-\x7E]/g, '')
  return ascii.slice(0, CAR_NAME_MAX)
}
function normalizeOemLabel(input: string): string {
  const ascii = input.replace(/[^\x20-\x7E]/g, '')
  return ascii.slice(0, OEM_LABEL_MAX)
}

type UsbEvent = { type?: string } & Record<string, unknown>
type DebouncedSave = DebouncedFunc<(s: ExtraConfig) => void>
type ToggleKey = 'autoPlay' | 'audioTransferMode' | 'nightMode' | 'kiosk'

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

export const Settings: React.FC = () => {
  const settings = useCarplayStore((s) => s.settings)
  const appContext = useContext(AppContext)
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

  const [draftWidth, setDraftWidth] = useState<string>(() => String(activeSettings.width))
  const [draftHeight, setDraftHeight] = useState<string>(() => String(activeSettings.height))
  useEffect(() => {
    setDraftWidth(String(activeSettings.width))
    setDraftHeight(String(activeSettings.height))
  }, [activeSettings.width, activeSettings.height])

  const [draftFps, setDraftFps] = useState<string>(() => String(activeSettings.fps ?? DEFAULT_FPS))
  useEffect(() => {
    setDraftFps(String(activeSettings.fps ?? DEFAULT_FPS))
  }, [activeSettings.fps])

  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)
  const theme = useTheme()
  const isDarkMode = theme.palette.mode === THEME.DARK
  const currentPrimary =
    (isDarkMode ? activeSettings.primaryColorDark : activeSettings.primaryColorLight) ??
    theme.palette.primary.main

  const debouncedSave = useMemo<DebouncedSave>(
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

  const getValidWifiChannel = (wifiType: ExtraConfig['wifiType'], ch?: number): number => {
    if (wifiType === WiFiValues['5ghz']) {
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
        const pending = requiresRestartParams.some((param) => {
          if (param === 'width') return String(draftWidth) !== String(prev.width)
          if (param === 'height') return String(draftHeight) !== String(prev.height)
          if (param === 'fps') return String(draftFps) !== String(prev.fps)
          return updated[param] !== prev[param]
        })
        setHasChanges(
          pending ||
            String(draftWidth) !== String(activeSettings.width) ||
            String(draftHeight) !== String(activeSettings.height) ||
            String(draftFps) !== String(activeSettings.fps)
        )
      } else {
        setHasChanges(false)
      }
    }
  }

  const isValidInt = (n: unknown) => Number.isFinite(n) && Math.floor(Number(n)) === Number(n)
  const validateResolutionOrDefault = (wRaw: string, hRaw: string) => {
    const wNum = Number(wRaw)
    const hNum = Number(hRaw)
    const wOk = isValidInt(wNum) && wNum >= MIN_WIDTH && wNum <= MAX_WIDTH
    const hOk = isValidInt(hNum) && hNum >= MIN_HEIGHT && hNum <= MAX_HEIGHT
    return {
      width: wOk ? Math.round(wNum) : DEFAULT_WIDTH,
      height: hOk ? Math.round(hNum) : DEFAULT_HEIGHT
    }
  }

  const validateFpsOrDefault = (fpsRaw: string) => {
    const n = Number(fpsRaw)
    const ok = isValidInt(n) && n >= MIN_FPS && n <= MAX_FPS
    return { fps: ok ? Math.round(n) : DEFAULT_FPS }
  }

  const handleSave = async () => {
    const needsReset = hasChanges || micResetPending

    const { width: finalW, height: finalH } = validateResolutionOrDefault(draftWidth, draftHeight)
    const { fps: finalFps } = validateFpsOrDefault(draftFps)
    const next: ExtraConfig = { ...activeSettings, width: finalW, height: finalH, fps: finalFps }

    try {
      debouncedSave.flush()
    } catch {}
    try {
      debouncedSave.cancel()
    } catch {}

    await saveSettings(next)
    startTransition(() => {
      setActiveSettings(next)
      setDraftWidth(String(next.width))
      setDraftHeight(String(next.height))
      setDraftFps(String(next.fps))
    })

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
        const v = Number.isFinite(n) ? Math.round(Math.max(MIN_HEIGHT, n)) : current.height
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
      case 'autoPlay':
        return Boolean(raw) as ExtraConfig[K]

      case 'wifiType': {
        const v =
          raw === WiFiValues['2.4ghz']
            ? WiFiValues['2.4ghz']
            : raw === WiFiValues['5ghz']
              ? WiFiValues['5ghz']
              : current.wifiType
        return v as ExtraConfig[K]
      }
      case 'micType': {
        const v = raw === 'box' ? 'box' : raw === 'os' ? 'os' : current.micType
        return v as ExtraConfig[K]
      }

      case 'mediaSound': {
        const n = Number(raw)
        const v = n === 0 ? 0 : 1
        return v as ExtraConfig[K]
      }

      case 'primaryColorDark':
      case 'primaryColorLight':
      case 'highlightEditableFieldLight':
      case 'highlightEditableFieldDark':
      case 'camera':
      case 'microphone':
      case 'carName':
      case 'oemName':
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

  const toWifiType = (s: string): ExtraConfig['wifiType'] =>
    s === WiFiValues['5ghz'] ? WiFiValues['5ghz'] : WiFiValues['2.4ghz']

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
    if (lower === 'system default' || lower === 'default') return 'OS default'
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
      } catch {}
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

  const handleCloseDialogByBackspace = (
    e: React.KeyboardEvent<HTMLDivElement>,
    cb: (status: boolean) => void
  ) => {
    if (e.key === 'Backspace') {
      e.preventDefault()
      e.stopPropagation()
      cb(false)
    }
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

  const wifiOptions = [WiFiValues['2.4ghz'], WiFiValues['5ghz']]
  const wifiValue = coerceSelectValue(
    (activeSettings.wifiType as unknown as string) ?? '',
    wifiOptions as unknown as string[]
  )

  const setBool =
    <K extends ToggleKey>(key: K) =>
    (_: React.ChangeEvent<HTMLInputElement>, checked: boolean) =>
      settingsChange(key, checked as unknown as ExtraConfig[K])

  const audioEnabled = !activeSettings.audioTransferMode
  const onAudioSwitch = (_e: React.ChangeEvent<HTMLInputElement>, checked: boolean) => {
    settingsChange('audioTransferMode', !checked as ExtraConfig['audioTransferMode'])
  }

  const openSelectOnEnter = useCallback((e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'Enter') return

    const root = e.currentTarget as HTMLElement
    const btn =
      root.querySelector<HTMLElement>('[role="button"][aria-haspopup="listbox"]') ??
      (root.matches('[role="button"][aria-haspopup="listbox"]') ? root : null)

    if (btn) {
      e.preventDefault()
      e.stopPropagation()
      btn.focus()
      btn.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true }))
      btn.click()
    }
  }, [])
  if (!hasSettings) return null

  return (
    <Box
      id="settings-root"
      className={theme.palette.mode === 'dark' ? 'App-header-dark' : 'App-header-light'}
      p={2}
      display="flex"
      flexDirection="column"
      height="calc(100vh - 64px)"
    >
      <div style={{ overflowX: 'auto', height: '100%' }} data-scrolled-wrapper>
        <Box
          sx={{
            flexGrow: 1,
            px: 1.5,
            py: 0.25,
            display: 'flex',
            flexDirection: 'column',
            gap: 2
          }}
        >
          <Box
            sx={{
              px: 1,
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'flex-start',
              gap: 3,
              flexWrap: 'nowrap'
            }}
          >
            <Grid container spacing={2} sx={{ px: 1, width: '100%' }}>
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
                      id="width"
                      size="small"
                      label="WIDTH"
                      type="number"
                      value={draftWidth}
                      onChange={(e) => {
                        if (appContext?.keyboardNavigation?.focusedElId !== 'width') return
                        setDraftWidth(e.target.value)
                        setHasChanges(true)
                      }}
                      slotProps={{
                        input: {
                          inputProps: { min: MIN_WIDTH, max: MAX_WIDTH, step: 1 },
                          endAdornment: <InputAdornment position="end">px</InputAdornment>
                        }
                      }}
                      sx={{
                        width: 136,
                        ...highlightEditableField({
                          isActive: appContext?.keyboardNavigation?.focusedElId === 'width',
                          isDarkMode
                        })
                      }}
                    />
                    <Typography sx={{ textAlign: 'center', fontSize: 22, lineHeight: 1 }}>
                      ×
                    </Typography>
                    <TextField
                      id="height"
                      size="small"
                      label="HEIGHT"
                      type="number"
                      value={draftHeight}
                      onChange={(e) => {
                        if (appContext?.keyboardNavigation?.focusedElId !== 'height') return
                        setDraftHeight(e.target.value)
                        setHasChanges(true)
                      }}
                      slotProps={{
                        input: {
                          inputProps: { min: MIN_HEIGHT, max: MAX_HEIGHT, step: 1 },
                          endAdornment: <InputAdornment position="end">px</InputAdornment>
                        }
                      }}
                      sx={{
                        width: 136,
                        ...highlightEditableField({
                          isActive: appContext?.keyboardNavigation?.focusedElId === 'height',
                          isDarkMode
                        })
                      }}
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
                      id="fps"
                      size="small"
                      label="FPS"
                      type="number"
                      value={draftFps}
                      onChange={(e) => {
                        if (appContext?.keyboardNavigation?.focusedElId !== 'fps') return
                        setDraftFps(e.target.value)
                        setHasChanges(true)
                      }}
                      slotProps={{
                        input: { inputProps: { min: MIN_FPS, max: MAX_FPS, step: 1 } }
                      }}
                      sx={{
                        width: 136,
                        ...highlightEditableField({
                          isActive: appContext?.keyboardNavigation?.focusedElId === 'fps',
                          isDarkMode
                        })
                      }}
                    />
                    <Box sx={{ width: 24, height: 1 }} />
                    <TextField
                      id="mediaDelay"
                      size="small"
                      label="MEDIA DELAY"
                      type="number"
                      value={activeSettings.mediaDelay}
                      onChange={(e) => {
                        if (appContext?.keyboardNavigation?.focusedElId !== 'mediaDelay') return
                        settingsChange('mediaDelay', Number(e.target.value))
                      }}
                      slotProps={{
                        input: {
                          inputProps: { min: MEDIA_DELAY_MIN, max: MEDIA_DELAY_MAX, step: 50 },
                          endAdornment: <InputAdornment position="end">ms</InputAdornment>
                        }
                      }}
                      sx={{
                        width: 136,
                        ...highlightEditableField({
                          isActive: appContext?.keyboardNavigation?.focusedElId === 'mediaDelay',
                          isDarkMode
                        })
                      }}
                    />
                  </Box>
                </Box>
              </Grid>

              <Grid size={{ xs: 12, sm: 6 }}>
                <SectionHeader>AUDIO SETTINGS</SectionHeader>

                <Stack spacing={1.5} sx={{ pl: 1.5, width: '100%' }}>
                  <FormControl fullWidth>
                    <FormLabel sx={{ typography: 'body2' }}>AUDIO VOLUME</FormLabel>
                    <Slider
                      aria-label="audioVolume"
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
                      sx={highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'audioVolume',
                        isDarkMode
                      })}
                    />
                  </FormControl>

                  <FormControl fullWidth>
                    <FormLabel sx={{ typography: 'body2' }}>NAV VOLUME</FormLabel>
                    <Slider
                      aria-label="navVolume"
                      size="small"
                      value={Math.round((activeSettings.navVolume ?? 1.0) * 100)}
                      min={0}
                      max={100}
                      step={5}
                      marks
                      valueLabelDisplay="auto"
                      onChange={(_, v) =>
                        typeof v === 'number' && settingsChange('navVolume', v / 100)
                      }
                      sx={highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'navVolume',
                        isDarkMode
                      })}
                    />
                  </FormControl>
                </Stack>
              </Grid>
            </Grid>
          </Box>

          {/* Left: icon switches | Right: form grid (never wrap) */}
          <Box
            sx={{
              px: 1,
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'flex-start',
              gap: 3,
              flexWrap: 'nowrap'
            }}
          >
            {/* Left: Paper (fixed width) */}
            <Box sx={{ pl: 1.5, flex: '0 0 auto', mt: 1.5 }}>
              <Paper
                variant="outlined"
                sx={(theme) => ({
                  p: 0.75,
                  borderRadius: 1,
                  borderColor: theme.palette.divider,
                  background: 'transparent',
                  width: 168
                })}
              >
                {(
                  [
                    {
                      key: 'autoPlay' as const,
                      title: 'Auto Play',
                      visualChecked: Boolean(activeSettings.autoPlay),
                      IconOn: PlayCircleOutline,
                      IconOff: PlayCircleOutline,
                      onChange: setBool('autoPlay')
                    },
                    {
                      key: 'audioTransferMode' as const,
                      title: 'Audio',
                      visualChecked: audioEnabled,
                      IconOn: VolumeUpOutlined,
                      IconOff: VolumeOffOutlined,
                      onChange: onAudioSwitch
                    },
                    {
                      key: 'nightMode' as const,
                      title: 'Dark Mode',
                      visualChecked: Boolean(activeSettings.nightMode),
                      IconOn: DarkModeOutlined,
                      IconOff: LightModeOutlined,
                      onChange: setBool('nightMode')
                    },
                    {
                      key: 'kiosk' as const,
                      title: 'Fullscreen',
                      visualChecked: Boolean(activeSettings.kiosk),
                      IconOn: FullscreenOutlined,
                      IconOff: FullscreenOutlined,
                      onChange: setBool('kiosk')
                    }
                  ] as const
                ).map((item, idx, arr) => {
                  const Icon = item.visualChecked ? item.IconOn : item.IconOff
                  return (
                    <React.Fragment key={item.key}>
                      <FormControlLabel
                        sx={(theme) => ({
                          m: 0,
                          px: 0.25,
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          minHeight: 32,
                          '&:has(.MuiSwitch-input:focus-visible)': {
                            outline: `2px solid ${themeColors.highlightFocusedFieldDark}`,
                            outlineOffset: 4,
                            borderRadius: 1
                          },
                          '&:has(.MuiSwitch-input:focus-visible) .MuiFormControlLabel-label': {
                            color: theme.palette.primary.main
                          }
                        })}
                        labelPlacement="end"
                        label={
                          <Tooltip title={item.title} enterDelay={150}>
                            <div style={{ display: 'flex', alignItems: 'center' }}>
                              <span
                                style={{
                                  fontSize: '0.85rem',
                                  display: 'inline-block',
                                  marginRight: '0.5rem'
                                }}
                              >
                                {item.title}
                              </span>
                              <Box
                                aria-label={item.title}
                                sx={(theme) => ({
                                  display: 'inline-flex',
                                  alignItems: 'center',
                                  justifyContent: 'center',
                                  width: 22,
                                  height: 22,
                                  borderRadius: 1,
                                  color: item.visualChecked
                                    ? theme.palette.primary.main
                                    : theme.palette.text.disabled,
                                  border: `1px solid ${theme.palette.divider}`
                                })}
                              >
                                <Icon fontSize="inherit" />
                              </Box>
                            </div>
                          </Tooltip>
                        }
                        control={
                          <Switch
                            size="small"
                            checked={item.visualChecked}
                            onChange={item.onChange}
                            sx={{ mx: 0 }}
                            slotProps={{ input: { 'aria-label': item.title } }}
                          />
                        }
                      />
                      {idx < arr.length - 1 && (
                        <Divider flexItem sx={{ my: 0.25, opacity: 0.08 }} />
                      )}
                    </React.Fragment>
                  )
                })}
              </Paper>
            </Box>

            {/* Right: form grid (flexible, shrinks instead of wrapping) */}
            <Box sx={{ flex: '1 1 auto', minWidth: 0 }}>
              <Grid
                container
                spacing={2}
                columns={12}
                alignItems="center"
                sx={{ width: '100%', minWidth: 0, mt: 1.5 }}
              >
                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="wifi"
                    size="small"
                    select
                    fullWidth
                    sx={{
                      minWidth: 0,
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'wifi',
                        isDarkMode
                      })
                    }}
                    label="WIFI"
                    value={wifiValue}
                    onKeyDown={openSelectOnEnter}
                    onChange={(e) => settingsChange('wifiType', toWifiType(e.target.value))}
                  >
                    <MenuItem value={WiFiValues['2.4ghz']}>2.4 GHz</MenuItem>
                    <MenuItem value={WiFiValues['5ghz']}>5 GHz</MenuItem>
                  </TextField>
                </Grid>

                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="mic"
                    size="small"
                    select
                    fullWidth
                    sx={{
                      minWidth: 0,
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'mic',
                        isDarkMode
                      })
                    }}
                    label="MICROPHONE"
                    value={activeSettings.micType}
                    onKeyDown={openSelectOnEnter}
                    onChange={(e) => settingsChange('micType', e.target.value as 'box' | 'os')}
                  >
                    <MenuItem
                      value="os"
                      disabled={micUnavailable && activeSettings.micType !== 'os'}
                    >
                      <Typography noWrap component="span" title={micLabel}>
                        {renderOsMicLabel(micLabel)}
                      </Typography>
                    </MenuItem>
                    <MenuItem value="box">BOX</MenuItem>
                  </TextField>
                </Grid>

                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="camera"
                    size="small"
                    select
                    fullWidth
                    sx={{
                      minWidth: 0,
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'camera',
                        isDarkMode
                      })
                    }}
                    label="CAMERA"
                    value={cameraValue}
                    onKeyDown={openSelectOnEnter}
                    onChange={(e) => settingsChange('camera', e.target.value)}
                  >
                    {cameraOptions.map((cam) => (
                      <MenuItem key={cam.deviceId || 'none'} value={cam.deviceId}>
                        {cam.label}
                      </MenuItem>
                    ))}
                  </TextField>
                </Grid>

                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="carName"
                    size="small"
                    fullWidth
                    label="CAR NAME"
                    value={activeSettings.carName ?? ''}
                    onChange={(e) => {
                      if (appContext?.keyboardNavigation?.focusedElId !== 'carName') return

                      const v = normalizeCarName(e.target.value)
                      settingsChange('carName', v)
                    }}
                    sx={{
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'carName',
                        isDarkMode
                      })
                    }}
                    slotProps={{
                      input: { inputProps: { maxLength: CAR_NAME_MAX } },
                      formHelperText: { sx: { textAlign: 'right', m: 0, mt: 0.5 } }
                    }}
                    helperText={`${activeSettings.carName?.length ?? 0}/${CAR_NAME_MAX}`}
                  />
                </Grid>

                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="uiLabel"
                    size="small"
                    fullWidth
                    label="UI LABEL"
                    value={activeSettings.oemName ?? ''}
                    onChange={(e) => {
                      if (appContext?.keyboardNavigation?.focusedElId !== 'uiLabel') return
                      const v = normalizeOemLabel(e.target.value)
                      settingsChange('oemName', v as unknown as ExtraConfig['oemName'])
                    }}
                    sx={{
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'uiLabel',
                        isDarkMode
                      })
                    }}
                    slotProps={{
                      input: { inputProps: { maxLength: OEM_LABEL_MAX } },
                      formHelperText: { sx: { textAlign: 'right', m: 0, mt: 0.5 } }
                    }}
                    helperText={`${activeSettings.oemName?.length ?? 0}/${OEM_LABEL_MAX}`}
                  />
                </Grid>

                <Grid
                  size={{ xs: 12, sm: 4 }}
                  sx={{ display: 'flex', alignItems: 'center', minWidth: 0 }}
                >
                  <TextField
                    id="frequency"
                    size="small"
                    select
                    fullWidth
                    sx={{
                      minWidth: 0,
                      ...highlightEditableField({
                        isActive: appContext?.keyboardNavigation?.focusedElId === 'frequency',
                        isDarkMode
                      })
                    }}
                    label="SAMPLING FREQUENCY"
                    value={
                      typeof activeSettings.mediaSound === 'number' ? activeSettings.mediaSound : 1
                    }
                    onKeyDown={openSelectOnEnter}
                    onChange={(e) => settingsChange('mediaSound', Number(e.target.value) as 0 | 1)}
                    helperText=" "
                  >
                    <MenuItem value={0}>44.1 kHz</MenuItem>
                    <MenuItem value={1}>48 kHz</MenuItem>
                  </TextField>
                </Grid>

                <Grid size={{ xs: 12, sm: 4 }} />
              </Grid>
            </Box>
          </Box>
        </Box>

        <Box
          bgcolor="transparent"
          display="flex"
          justifyContent="center"
          sx={{
            position: 'relative',
            top: '-1rem',
            left: '-1.25rem',
            justifyContent: 'flex-end',
            pt: 1,
            pb: 1
          }}
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
          keepMounted
          onClose={() => setOpenBindings(false)}
          onKeyDown={(e) => handleCloseDialogByBackspace(e, setOpenBindings)}
          slots={{ transition: Transition }}
          slotProps={{ paper: { sx: { minHeight: '80%', minWidth: '80%' } } }}
        >
          <DialogTitle>Key Bindings</DialogTitle>
          <DialogContent>
            <KeyBindings settings={activeSettings} updateKey={settingsChange} />
          </DialogContent>
        </Dialog>

        <Dialog
          open={openAdvanced}
          keepMounted
          onClose={() => setOpenAdvanced(false)}
          onKeyDown={(e) => handleCloseDialogByBackspace(e, setOpenAdvanced)}
          maxWidth={false}
          slots={{ transition: Transition }}
          slotProps={{
            paper: {
              sx: {
                width: 320,
                maxWidth: 'calc(100vw - 48px)',
                borderRadius: 2
              }
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
                type="color"
                value={currentPrimary}
                onChange={(e) => {
                  const next = e.target.value
                  setActiveSettings((prev) => {
                    const updated = {
                      ...prev,
                      [isDarkMode ? 'primaryColorDark' : 'primaryColorLight']: next,
                      [isDarkMode ? 'highlightEditableFieldDark' : 'highlightEditableFieldLight']:
                        next
                    } as ExtraConfig
                    debouncedSave(updated)
                    return updated
                  })
                }}
                slotProps={{
                  inputLabel: { shrink: true },
                  input: {
                    endAdornment: (
                      <InputAdornment position="end">
                        <Button
                          size="small"
                          variant="outlined"
                          onClick={() => {
                            setActiveSettings((prev) => {
                              const updated = {
                                ...prev,
                                [isDarkMode ? 'primaryColorDark' : 'primaryColorLight']: undefined,
                                [isDarkMode
                                  ? 'highlightEditableFieldDark'
                                  : 'highlightEditableFieldLight']: undefined
                              } as ExtraConfig
                              debouncedSave(updated)
                              return updated
                            })
                          }}
                          sx={{ ml: 1, py: 0.25, px: 1 }}
                        >
                          RESET
                        </Button>
                      </InputAdornment>
                    )
                  }
                }}
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
                slotProps={{
                  inputLabel: { shrink: true },
                  input: { inputProps: { tabIndex: openAdvanced ? 0 : -1 } }
                }}
                autoFocus={openAdvanced}
                sx={{ width: 120 }}
              />
              <TextField
                label="FORMAT"
                type="number"
                size="small"
                margin="dense"
                value={activeSettings.format}
                onChange={(e) => settingsChange('format', Number(e.target.value))}
                slotProps={{ inputLabel: { shrink: true } }}
                sx={{ width: 120 }}
              />
            </Box>
          </DialogContent>
        </Dialog>
      </div>
    </Box>
  )
}
