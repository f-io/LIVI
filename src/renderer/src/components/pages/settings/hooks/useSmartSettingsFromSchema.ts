import { useCallback, useMemo } from 'react'
import { useSmartSettings } from './useSmartSettings'
import { getValueByPath } from '../utils'
import type { SettingsNode } from '../../../../routes'
import type { ExtraConfig } from '@main/Globals'

type FlatSettings = Record<string, any>

type Overrides = Record<
  string,
  {
    transform?: (value: any, prev?: any) => any
    validate?: (value: any) => boolean
  }
>

const walkSchema = (
  node: SettingsNode<ExtraConfig>,
  settings: any,
  initial: FlatSettings,
  overrides: Overrides
) => {
  if (node.type !== 'route') {
    const path = (node as any).path as unknown

    if (typeof path === 'string' && path.length > 0) {
      initial[path] = getValueByPath(settings, path)

      const transform = (node as any).transform
      if (typeof transform === 'function') {
        overrides[path] = { transform }
      }
    }
  }

  if (node.type === 'route') {
    node.children.forEach((child) => walkSchema(child, settings, initial, overrides))
  }
}

export const useSmartSettingsFromSchema = (
  rootSchema: SettingsNode<ExtraConfig>,
  settings: any
) => {
  const { initialState, overrides } = useMemo(() => {
    const initialState: FlatSettings = {}
    const overrides: Overrides = {}

    walkSchema(rootSchema, settings ?? {}, initialState, overrides)

    return { initialState, overrides }
  }, [rootSchema, settings])

  const smart = useSmartSettings(initialState, settings ?? {}, { overrides }) as any

  const requestRestart = useCallback(() => {
    if (typeof smart?.requestRestart === 'function') {
      smart.requestRestart()
    }
  }, [smart])

  return {
    ...smart,
    requestRestart
  }
}
