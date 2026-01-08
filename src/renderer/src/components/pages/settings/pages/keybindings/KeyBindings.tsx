import { KeyBindings as KeyBindingsComponent } from './KeyBindingsComponent'
// import { useCarplayStore } from '@store/store'

export const KeyBindings = ({ state }) => {
  // const settings = useCarplayStore((s) => s.settings)
  // const handleSave = () => {}
  const handleUpdateKey = () => {}

  return <KeyBindingsComponent state={state} updateKey={handleUpdateKey} />
}
