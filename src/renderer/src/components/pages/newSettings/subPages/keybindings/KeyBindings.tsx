import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { KeyBindings as KeyBindingsComponent } from './KeyBindingsComponent'
import { useCarplayStore } from '@store/store'

export const KeyBindings = () => {
  const settings = useCarplayStore((s) => s.settings)
  const handleSave = () => {}
  const handleUpdateKey = () => {}

  return (
    <SettingsLayout onSave={handleSave}>
      <KeyBindingsComponent settings={settings} updateKey={handleUpdateKey} />
    </SettingsLayout>
  )
}
