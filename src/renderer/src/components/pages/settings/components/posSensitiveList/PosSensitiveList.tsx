import { IconButton, Switch, Typography } from '@mui/material'
import ExpandLessIcon from '@mui/icons-material/ExpandLess'
import ExpandMoreIcon from '@mui/icons-material/ExpandMore'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { StackItem } from '../stackItem'
import type { PosListNode } from '@renderer/routes/types'
import type { TelemetryDashboardConfig, TelemetryDashboardId } from '@main/Globals'

type Props<T> = {
  node: PosListNode
  value: T
  onChange: (v: T) => void
}

const isDashId = (id: string): id is TelemetryDashboardId =>
  id === 'dash1' || id === 'dash2' || id === 'dash3' || id === 'dash4'

const defaultDashboardsFromNode = (node: PosListNode): TelemetryDashboardConfig[] => {
  const ids = node.items.map((i) => i.id).filter(isDashId)
  return ids.map((id, idx) => ({ id, enabled: false, pos: idx + 1 }))
}

const iconSx = {
  fontSize: 'clamp(22px, 4.2vh, 34px)'
} as const

const btnSx = {
  padding: 'clamp(4px, 1.2vh, 10px)'
} as const

const normalizeDashboards = (node: PosListNode, raw: unknown): TelemetryDashboardConfig[] => {
  const base = defaultDashboardsFromNode(node)

  // nothing stored yet -> defaults
  if (!Array.isArray(raw)) return base

  // merge stored values into defaults
  const map = new Map<TelemetryDashboardId, TelemetryDashboardConfig>()
  for (const d of base) map.set(d.id, d)

  for (const v of raw) {
    if (!v || typeof v !== 'object') continue
    const obj = v as Partial<TelemetryDashboardConfig> & { id?: unknown }

    if (typeof obj.id !== 'string' || !isDashId(obj.id)) continue

    const prev = map.get(obj.id)
    if (!prev) continue

    map.set(obj.id, {
      id: obj.id,
      enabled: Boolean(obj.enabled),
      pos: typeof obj.pos === 'number' && Number.isFinite(obj.pos) ? Math.round(obj.pos) : prev.pos
    })
  }

  // ensure unique + stable positions: sort by pos then renumber 1..n
  const sorted = Array.from(map.values()).sort((a, b) => a.pos - b.pos)
  return sorted.map((d, idx) => ({ ...d, pos: idx + 1 }))
}

export const PosSensitiveList = <T,>({ node, value, onChange }: Props<T>) => {
  const { t } = useTranslation()

  const dashboards = useMemo(() => {
    return normalizeDashboards(node, value as unknown)
  }, [node, value])

  const commit = (next: TelemetryDashboardConfig[]) => {
    const sorted = next.slice().sort((a, b) => a.pos - b.pos)
    onChange(sorted as unknown as T)
  }

  const move = (id: TelemetryDashboardId, dir: 'up' | 'down') => {
    const sorted = dashboards.slice().sort((a, b) => a.pos - b.pos)
    const idx = sorted.findIndex((d) => d.id === id)
    if (idx < 0) return

    const swapWith = dir === 'up' ? idx - 1 : idx + 1
    if (swapWith < 0 || swapWith >= sorted.length) return

    const a = sorted[idx]
    const b = sorted[swapWith]

    const next = sorted.map((d) => {
      if (d.id === a.id) return { ...d, pos: b.pos }
      if (d.id === b.id) return { ...d, pos: a.pos }
      return d
    })

    commit(next)
  }

  const setEnabled = (id: TelemetryDashboardId, enabled: boolean) => {
    const next = dashboards.map((d) => (d.id === id ? { ...d, enabled } : d))
    commit(next)
  }

  const sorted = dashboards.slice().sort((a, b) => a.pos - b.pos)

  return (
    <>
      {sorted.map((d, index) => {
        const meta = node.items.find((i) => i.id === d.id)
        const label = meta ? (meta.labelKey ? t(meta.labelKey, meta.label) : meta.label) : d.id

        const canUp = index > 0
        const canDown = index < sorted.length - 1

        return (
          <StackItem key={d.id}>
            <Typography>{label}</Typography>

            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <IconButton sx={btnSx} disabled={!canUp} onClick={() => move(d.id, 'up')}>
                <ExpandLessIcon sx={iconSx} />
              </IconButton>

              <IconButton sx={btnSx} disabled={!canDown} onClick={() => move(d.id, 'down')}>
                <ExpandMoreIcon sx={iconSx} />
              </IconButton>

              <Switch checked={Boolean(d.enabled)} onChange={(_, v) => setEnabled(d.id, v)} />
            </div>
          </StackItem>
        )
      })}
    </>
  )
}
