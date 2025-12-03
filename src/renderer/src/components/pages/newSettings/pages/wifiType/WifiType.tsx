import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@store/store'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { WiFiTypeSettingKey } from './types'
import { SettingsItemRow } from '../../components'
import { WiFiValues } from '../../../settings/constants'

export const WifiType: React.FC = () => {
  // FIXME types
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<WiFiTypeSettingKey, number | string | boolean> = {
    wifiType: settings?.wifiType
  }

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)

  const config = {
    [WiFiValues['2.4ghz']]: { label: WiFiValues['2.4ghz'], type: 'checkbox' },
    [WiFiValues['5ghz']]: { label: WiFiValues['5ghz'], type: 'checkbox' }
  }

  return (
    <SettingsLayout onSave={save}>
      {Object.keys(config).map((item, index) => {
        return (
          <SettingsItemRow
            key={index}
            config={config}
            item={item}
            state={settingsState}
            transformer={() => {
              return settingsState.wifiType === item
            }}
            onClick={(e, value) => handleFieldChange(item, value)}
            onChange={(e, value) => handleFieldChange(item, value)}
          />
        )
      })}

      <span>
        Connected to WiFi, view available networks, and manage network settings from the Network
      </span>
    </SettingsLayout>
  )
}
