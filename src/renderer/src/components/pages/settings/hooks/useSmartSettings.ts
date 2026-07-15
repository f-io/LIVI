import { useLiviStore, useStatusStore } from '@store/store'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { requiresRestartParams } from '../constants'
import { getValueByPath, setValueByPath } from '../utils'

type OverrideConfig = {
  transform?: (value: unknown, prev: unknown) => unknown
  validate?: (value: unknown) => boolean
}

type Overrides = Record<string, OverrideConfig>

function isRestartRelevantPath(path?: string) {
  if (!path) return true
  return !(path === 'bindings' || path.startsWith('bindings.'))
}

export function useSmartSettings<T extends Record<string, unknown>>(
  initial: T,
  settings: T,
  options?: { overrides?: Overrides }
) {
  const overrides = options?.overrides ?? {}
  const [state, setState] = useState<T>(() => ({ ...initial }))
  const [restartRequested, setRestartRequested] = useState(false)

  const saveSettings = useLiviStore((s) => s.saveSettings)
  const restartBaseline = useLiviStore((s) => s.restartBaseline)
  const markRestartBaseline = useLiviStore((s) => s.markRestartBaseline)
  const isDongleHardwarePresent = useStatusStore((s) => s.isDongleHardwarePresent)
  const activeProtocol = useStatusStore((s) => s.activeProtocol)
  const wirelessAaEnabled = useLiviStore((s) => Boolean(s.settings?.wirelessAaEnabled))
  const wirelessCpEnabled = useLiviStore((s) => Boolean(s.settings?.wirelessCpEnabled))

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
    const cfg = (settings ?? {}) as Record<string, unknown>
    const baseline = (restartBaseline ?? settings ?? {}) as Record<string, unknown>

    for (const key of requiresRestartParams) {
      if (!isRestartRelevantPath(key)) continue
      if (JSON.stringify(cfg[key]) !== JSON.stringify(baseline[key])) return true
    }
    return false
  }, [settings, restartBaseline])

  const needsRestart = useMemo(() => {
    return Boolean(needsRestartFromConfig || restartRequested)
  }, [needsRestartFromConfig, restartRequested])

  const requestRestart = useCallback((path?: string) => {
    if (!isRestartRelevantPath(path)) return
    setRestartRequested(true)
  }, [])

  const handleFieldChange = (path: string, rawValue: unknown) => {
    const prevValue = state[path]
    const override = overrides[path]

    const nextValue = override?.transform?.(rawValue, prevValue) ?? rawValue
    if (override?.validate && !override.validate(nextValue)) return

    setState((prev) => {
      const next = { ...prev, [path]: nextValue }

      const newSettings = structuredClone((settings ?? {}) as T)
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

    const nativeOrWireless =
      wirelessAaEnabled ||
      wirelessCpEnabled ||
      activeProtocol === 'androidauto' ||
      activeProtocol === 'carplay'

    if (nativeOrWireless) {
      await window.projection.ipc.restart()
    } else if (isDongleHardwarePresent) {
      await window.projection.usb.forceReset()
    } else {
      return false
    }

    markRestartBaseline()
    setRestartRequested(false)

    return true
  }

  return {
    state,
    isDirty,
    needsRestart,
    handleFieldChange,
    resetState,
    restart,
    requestRestart
  }
}
