import { ExtraConfig } from "../../../main/Globals"
import React, { useEffect, useMemo, useState } from "react"
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
  InputAdornment,
} from '@mui/material'
import { useTheme } from '@mui/material/styles'
import type { SxProps, Theme } from '@mui/material/styles'
import { TransitionProps } from '@mui/material/transitions'
import { KeyBindings } from "./KeyBindings"
import { useCarplayStore, useStatusStore } from "../store/store"
import { updateCameras as detectCameras } from '../utils/cameraDetection'
import debounce from 'lodash.debounce'

interface SettingsProps {
  settings: ExtraConfig | null
}

const Transition = React.forwardRef(function Transition(
  props: TransitionProps & { children: React.ReactElement },
  ref: React.Ref<unknown>,
) {
  return <Slide direction="up" ref={ref} {...props} />
})

const SectionHeader: React.FC<{ children: React.ReactNode; sx?: SxProps<Theme> }> = ({ children, sx }) => {
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
          borderRadius: 1,
        },
        ...sx,
      }}
    >
      {children}
    </Typography>
  )
}

const Settings: React.FC<SettingsProps> = ({ settings }) => {
  if (!settings) return null

  const [activeSettings, setActiveSettings] = useState<ExtraConfig>({
    ...settings,
    audioVolume: settings.audioVolume ?? 1.0,
    navVolume: settings.navVolume ?? 1.0,
  })
  const [micLabel, setMicLabel] = useState('not available')
  const [cameras, setCameras] = useState<MediaDeviceInfo[]>([])
  const [openBindings, setOpenBindings] = useState(false)
  const [isResetting, setIsResetting] = useState(false)
  const [resetMessage, setResetMessage] = useState("")
  const [closeCountdown, setCloseCountdown] = useState(0)
  const [hasChanges, setHasChanges] = useState(false)
  const [openAdvanced, setOpenAdvanced] = useState(false)

  const saveSettings = useCarplayStore(s => s.saveSettings)
  const isDongleConnected = useStatusStore(s => s.isDongleConnected)
  const setCameraFound = useStatusStore(s => s.setCameraFound)
  const theme = useTheme()
  const isDarkMode = theme.palette.mode === 'dark'
  const currentPrimary =
    (isDarkMode ? activeSettings.primaryColorDark : activeSettings.primaryColorLight)
    ?? theme.palette.primary.main

  const debouncedSave = useMemo(() => debounce((newSettings: ExtraConfig) => saveSettings(newSettings), 300), [saveSettings])
  useEffect(() => () => debouncedSave.cancel(), [debouncedSave])

  const requiresRestartParams: (keyof ExtraConfig)[] = [
    'width', 'height', 'fps', 'dpi', 'format', 'mediaDelay', 'wifiType', 'micType', 'audioTransferMode'
  ]

  const getValidWifiChannel = (wifiType: ExtraConfig['wifiType'], ch?: number): number => {
    if (wifiType === '5ghz') {
      return typeof ch === 'number' && ch >= 36 ? ch : 36
    }
    return typeof ch === 'number' && ch > 0 && ch < 36 ? ch : 6
  }

  const settingsChange = (key: keyof ExtraConfig, value: any) => {
    let updated: ExtraConfig = { ...activeSettings, [key]: value }

    if (key === 'wifiType') {
      updated = {
        ...updated,
        wifiChannel: getValidWifiChannel(value as ExtraConfig['wifiType'], updated.wifiChannel)
      }
    }

    setActiveSettings(updated)

    if (['audioVolume', 'navVolume'].includes(key)) {
      debouncedSave(updated)
    } else if (['kiosk', 'nightMode'].includes(key)) {
      saveSettings(updated)
    } else if (requiresRestartParams.includes(key)) {
      const pending = requiresRestartParams.some(p => updated[p] !== settings[p])
      setHasChanges(pending)
    } else {
      saveSettings(updated)
    }
  }

  const handleSave = async () => {
    setIsResetting(true)
    setCloseCountdown(3)

    let resetStatus = ""
    try {
      if (isDongleConnected) {
        setResetMessage("Dongle Reset...")
        const ok = await window.carplay.usb.forceReset()
        resetStatus = ok ? "Success" : "Failed"
      } else {
        resetStatus = "Settings saved (no dongle connected)"
      }
    } catch {
      resetStatus = "Dongle Reset Error."
    }

    await saveSettings(activeSettings)
    setHasChanges(false)
    setIsResetting(false)
    setResetMessage(resetStatus)
  }

  useEffect(() => {
    if (!resetMessage) return
    const timerId = setInterval(() => {
      setCloseCountdown(prev => {
        if (prev <= 1) {
          clearInterval(timerId)
          setResetMessage("")
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

  useEffect(() => {
    const updateMic = async () => {
      try {
        const label = await window.carplay.usb.getSysdefaultPrettyName()
        const final = label && !['sysdefault', 'null'].includes(label) ? label : 'not available'
        setMicLabel(final)

        if (final === 'not available' && activeSettings.micType === 'os') {
          const upd: ExtraConfig = { ...activeSettings, micType: 'box' as const }
          setActiveSettings(upd)
          debouncedSave(upd)
        } else if (final !== 'not available' && !activeSettings.micType) {
          const upd: ExtraConfig = { ...activeSettings, micType: 'os' as const }
          setActiveSettings(upd)
          debouncedSave(upd)
        }
      } catch {
        console.warn('[Settings] Mic label fetch failed')
      }
    }
    updateMic()
    const micUsbHandler = (_: any, data: { type: string }) => {
      if (['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) updateMic()
    }
    window.carplay.usb.listenForEvents(micUsbHandler)
  }, [activeSettings, debouncedSave])

  useEffect(() => {
    detectCameras(setCameraFound, saveSettings, activeSettings).then(setCameras)
    const usbHandler = (_: any, data: { type: string }) => {
      if (['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        detectCameras(setCameraFound, saveSettings, activeSettings).then(setCameras)
      }
    }
    window.carplay.usb.listenForEvents(usbHandler)
  }, [])

  useEffect(() => {
    let off: (() => void) | undefined
      ; (async () => {
        try {
          const kiosk = await window.app.getKiosk()
          setActiveSettings(prev => (prev.kiosk === kiosk ? prev : { ...prev, kiosk }))
        } catch { }
        off = window.app.onKioskSync(kiosk => {
          setActiveSettings(prev => (prev.kiosk === kiosk ? prev : { ...prev, kiosk }))
        })
      })()
    return () => { if (off) off() }
  }, [])

  const handleClosePopup = () => {
    setResetMessage("")
    setCloseCountdown(0)
  }

  const micUnavailable = micLabel === 'not available'

  return (
    <Box
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
          gap: 2,
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
                  width: 'fit-content',
                }}
              >
                <TextField
                  size="small"
                  label="WIDTH"
                  type="number"
                  value={activeSettings.width}
                  onChange={e => settingsChange('width', Number(e.target.value))}
                  sx={{ width: 136 }}
                />
                <Typography sx={{ textAlign: 'center', fontSize: 22, lineHeight: 1 }}>×</Typography>
                <TextField
                  size="small"
                  label="HEIGHT"
                  type="number"
                  value={activeSettings.height}
                  onChange={e => settingsChange('height', Number(e.target.value))}
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
                  width: 'fit-content',
                }}
              >
                <TextField
                  size="small"
                  label="FPS"
                  type="number"
                  value={activeSettings.fps}
                  onChange={e => settingsChange('fps', Number(e.target.value))}
                  sx={{ width: 136 }}
                />
                <Box sx={{ width: 24, height: 1 }} />
                <TextField
                  size="small"
                  label="MEDIA DELAY"
                  type="number"
                  value={activeSettings.mediaDelay}
                  onChange={e => settingsChange('mediaDelay', Number(e.target.value))}
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
                  onChange={(_, v) => typeof v === 'number' && settingsChange('audioVolume', v / 100)}
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

        <Grid
          container
          spacing={2}
          sx={{ px: 1 }}
          columns={12}
          alignItems="center"
        >
          <Grid size={{ xs: 6, sm: 3 }}>
            <Stack spacing={0.5}>
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.kiosk}
                    onChange={e => settingsChange('kiosk', e.target.checked)}
                  />
                }
                label="KIOSK"
              />
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.nightMode}
                    onChange={e => settingsChange('nightMode', e.target.checked)}
                  />
                }
                label="DARK MODE"
              />
              <FormControlLabel
                control={
                  <Checkbox
                    size="small"
                    checked={activeSettings.audioTransferMode}
                    onChange={e => settingsChange('audioTransferMode', e.target.checked)}
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
              value={activeSettings.wifiType}
              onChange={e => settingsChange('wifiType', e.target.value)}
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
              onChange={e => settingsChange('micType', e.target.value as 'box' | 'os')}
              helperText={micUnavailable ? 'System mic not available' : undefined}
            >
              <MenuItem value="os" disabled={micUnavailable}>
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
              value={activeSettings.camera ?? ''}
              onChange={e => settingsChange('camera', e.target.value)}
            >
              {(cameras.length ? cameras : [{ deviceId: '', label: 'No camera' }]).map((cam: any) => (
                <MenuItem key={cam.deviceId ?? 'none'} value={cam.deviceId ?? ''}>
                  {cam.label || 'Camera'}
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
            color={hasChanges ? 'primary' : 'inherit'}
            onClick={hasChanges ? handleSave : undefined}
            disabled={!hasChanges || isResetting}
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
          <Typography variant="body1" sx={{ mb: 2 }}>{resetMessage}</Typography>
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
            sx={theme => ({
              display: 'grid',
              gridTemplateColumns: '120px 120px',
              columnGap: theme.spacing(1.5),
              rowGap: theme.spacing(1.5),
              justifyContent: 'center',
            })}
          >
            <TextField
              size="small"
              label={isDarkMode ? 'PRIMARY (DARK)' : 'PRIMARY (LIGHT)'}
              value={currentPrimary}
              onChange={e =>
                settingsChange(isDarkMode ? 'primaryColorDark' : 'primaryColorLight', e.target.value)
              }
              InputProps={{
                inputProps: { type: 'color' },
                endAdornment: (
                  <InputAdornment position="end">
                    <Button
                      size="small"
                      variant="outlined"
                      onClick={() =>
                        settingsChange(isDarkMode ? 'primaryColorDark' : 'primaryColorLight', undefined)
                      }
                      sx={{ ml: 1, py: 0.25, px: 1 }}
                    >
                      RESET
                    </Button>
                  </InputAdornment>
                ),
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
              onChange={e => settingsChange('dpi', Number(e.target.value))}
              InputLabelProps={{ shrink: true }}
              autoFocus
              sx={{ width: 120 }}
            />
            <TextField
              label="FORMAT"
              type="number"
              size="small"
              margin="dense"
              value={activeSettings.format}
              onChange={e => settingsChange('format', Number(e.target.value))}
              InputLabelProps={{ shrink: true }}
              sx={{ width: 120 }}
            />
          </Box>
        </DialogContent>
      </Dialog>
    </Box>
  )
}

export default Settings
