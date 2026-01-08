import { useMemo, useState, useEffect, useCallback } from 'react'
import { requiresRestartParams } from '../constants'
import { getValueByPath, setValueByPath } from '../utils'
import { useCarplayStore, useStatusStore } from '@store/store'

type OverrideConfig<T> = {
  transform?: (value: any, prev: T) => T
  validate?: (value: any) => boolean
}

type Overrides<T> = Partial<Record<keyof T, OverrideConfig<T>>>

export function useSmartSettings<T extends Record<string, any>>(
  initial: T,
  settings: T,
  options?: { overrides?: Overrides<T> }
) {
  const overrides = options?.overrides ?? {}
  const [state, setState] = useState<T>(() => ({ ...initial }))
  const [restartRequested, setRestartRequested] = useState(false)

  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const restartBaseline = useCarplayStore((s) => s.restartBaseline)
  const markRestartBaseline = useCarplayStore((s) => s.markRestartBaseline)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)

  useEffect(() => {
    setState({ ...initial })
  }, [initial])

  const isDirty = useMemo(
    () =>
      Object.keys(state).some((path) => {
        return getValueByPath(settings, path) !== state[path]
      }),
    [state, settings]
  )

  const needsRestartFromConfig = useMemo(() => {
    const cfg = (settings ?? {}) as any
    const baseline = (restartBaseline ?? settings ?? {}) as any

    for (const key of requiresRestartParams) {
      if (cfg[key] !== baseline[key]) return true
    }
    return false
  }, [settings, restartBaseline])

  const needsRestart = useMemo(() => {
    return Boolean(needsRestartFromConfig || restartRequested)
  }, [needsRestartFromConfig, restartRequested])

  const requestRestart = useCallback(() => {
    setRestartRequested(true)
  }, [])

  const handleFieldChange = (path: string, rawValue: any) => {
    const prevValue = state[path]
    const override = overrides[path]

    const nextValue = override?.transform?.(rawValue, prevValue) ?? rawValue
    if (override?.validate && !override.validate(nextValue)) return

    setState((prev) => {
      const next = { ...prev, [path]: nextValue }

      const newSettings = structuredClone((settings ?? {}) as any)
      Object.entries(next).forEach(([p, v]) => {
        setValueByPath(newSettings, p, v)
      })

      saveSettings(newSettings)
      return next
    })
  }

  const resetState = () => setState(initial)

  const restart = async () => {
    if (!needsRestart) return false
    if (!isDongleConnected) return false

    await window.carplay.usb.forceReset()

    markRestartBaseline()
    setRestartRequested(false)

    return true
  }

  return {
    state,
    isDirty,
    needsRestart,
    isDongleConnected,
    handleFieldChange,
    resetState,
    restart,
    requestRestart
  }
}
