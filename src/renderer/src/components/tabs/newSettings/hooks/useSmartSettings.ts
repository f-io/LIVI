import { useState } from 'react'
import { requiresRestartParams } from '../../settings/constants'
// import { useCarplayStore } from '@store/store'

type OverrideConfig<T> = {
  transform?: (value: any, prev: T) => T
  validate?: (value: any) => boolean
}

type Overrides<T> = Partial<Record<keyof T, OverrideConfig<T>>>

export function useSmartSettings<T extends Record<string, any>, K extends keyof T>(
  initial: T,
  settings: T,
  options?: {
    overrides?: Overrides<T>
  }
) {
  // const saveSettings = useCarplayStore((s) => s.saveSettings)
  const overrides = options?.overrides ?? {}

  const [state, setState] = useState<T>(initial)
  const [isRequireReset, setIsRequireReset] = useState(false)

  const evaluateNeedRestart = (key: K, nextValue: T[K]) => {
    return requiresRestartParams.includes(key) && settings[key] !== nextValue
  }

  const handleFieldChange = <K extends keyof T>(key: K, rawValue: T[K] | any) => {
    const prevValue = state[key]

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    const override: OverrideConfig<T[K]> | undefined = overrides[key]

    // transform
    const transformedValue = override?.transform?.(rawValue, prevValue) ?? rawValue

    // validate
    if (override?.validate && !override.validate(transformedValue)) return

    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    const needRestart = evaluateNeedRestart(key, transformedValue)

    setIsRequireReset(needRestart)
    setState((prev) => ({ ...prev, [key]: transformedValue }))
  }

  const resetState = () => {
    setState(initial)
    setIsRequireReset(false)
  }

  const save = () => {
    const newSettings = { ...settings, ...overrides }

    console.log('saved settings:', newSettings)

    // saveSettings(newSettings)
    return isRequireReset
  }

  return {
    state,
    isRequireReset,
    handleFieldChange,
    resetState,
    save
  }
}
