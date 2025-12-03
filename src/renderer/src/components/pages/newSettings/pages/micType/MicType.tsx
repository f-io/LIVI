import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@store/store'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { MicTypeSettingKey } from './types'
import { MicTypeValues } from '../../../settings/constants'
import { SettingsItemRow } from '../../components'

export const MicType: React.FC = () => {
  // FIXME types
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<MicTypeSettingKey, number | string | boolean> = {
    micType: settings?.micType
  }

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)

  const config = {
    [MicTypeValues['box']]: { label: MicTypeValues['box'], type: 'checkbox' },
    [MicTypeValues['os']]: { label: MicTypeValues['os'], type: 'checkbox' }
  }

  console.log(settingsState)

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
              return settingsState.micType === item
            }}
            onClick={(e, value) => handleFieldChange(item, value)}
            onChange={(e, value) => handleFieldChange(item, value)}
          />
        )
      })}

      <span>Helper text</span>
    </SettingsLayout>
  )
}
