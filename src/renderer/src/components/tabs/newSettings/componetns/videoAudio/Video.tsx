import { SettingsLayout } from '@renderer/components/layouts'
import { Typography } from '@mui/material'
import { StackItem } from '../StackItem'
import { useCarplayStore } from '@store/store'
import { dialogsConfig } from '../dialog/config'
import { useSettingsDialogs } from '../../hooks'
import { VideoSettingKey } from './types'
import { videoSettingsUIConfig } from './config'
import { useSmartSettings } from '../../hooks/useSmartSettings'

export const Video = () => {
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<VideoSettingKey, number> = {
    width: settings?.width ?? 0,
    height: settings?.height ?? 0,
    fps: settings?.fps ?? 0,
    mediaDelay: settings?.mediaDelay ?? 0,
    audioVolume: settings?.audioVolume ?? 0,
    navVolume: settings?.navVolume ?? 0
  }

  const {
    state: settingsState,
    handleFieldChange,
    resetState,
    save
  } = useSmartSettings(initialState, settings)

  const { dialog, onToggleDialog } = useSettingsDialogs({
    dialogsConfig,
    data: settingsState,
    onSave: save,
    onChange: handleFieldChange,
    onReset: resetState
  })

  const stackItems = (Object.keys(videoSettingsUIConfig) as VideoSettingKey[])
    .filter((key) => key !== 'height')
    .map((key) => {
      const cfg = videoSettingsUIConfig[key]
      return (
        <StackItem key={key} onClick={() => cfg.dialog && onToggleDialog(cfg.dialog!, true)}>
          <Typography>{cfg.label}</Typography>
          <Typography>{cfg.display ? cfg.display(settingsState) : settingsState[key]}</Typography>
        </StackItem>
      )
    })

  return (
    <SettingsLayout onSave={save}>
      {stackItems}
      {dialog && dialog()}
    </SettingsLayout>
  )
}
