import { SettingsNode } from '../../../routes'
import { ExtraConfig } from '@main/Globals'

type AnyRecord = Record<string, unknown>

function isRecord(v: unknown): v is AnyRecord {
  return typeof v === 'object' && v !== null
}

export const getValueByPath = (obj: unknown, path: string): unknown => {
  if (!path) return undefined

  const keys = path.split('.').filter(Boolean)
  let cur: unknown = obj

  for (const key of keys) {
    if (!isRecord(cur)) return undefined
    cur = cur[key]
  }

  return cur
}

export const setValueByPath = (obj: AnyRecord, path: string, value: unknown): void => {
  const keys = path.split('.').filter(Boolean)
  if (keys.length === 0) return

  let cur: AnyRecord = obj

  for (const k of keys.slice(0, -1)) {
    const next = cur[k]
    if (isRecord(next)) {
      cur = next
    } else {
      const created: AnyRecord = {}
      cur[k] = created
      cur = created
    }
  }

  cur[keys[keys.length - 1]] = value
}

export const getNodeByPath = (
  root: SettingsNode<ExtraConfig>,
  segments: string[]
): SettingsNode<ExtraConfig> | null => {
  let current: SettingsNode<ExtraConfig> | null = root

  for (let i = 0; i < segments.length; i++) {
    if (!current || current.type !== 'route') return null

    const segment = segments[i]

    const routeChild = current.children.find((c) => c.type === 'route' && c.route === segment)

    if (routeChild) {
      current = routeChild
      continue
    }

    const leafChild = current.children.find((c) => 'path' in c && c.path === segment)

    if (leafChild) {
      return leafChild
    }

    return null
  }

  return current
}
