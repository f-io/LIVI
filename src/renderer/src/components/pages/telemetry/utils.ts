import type { Dashboards, Screen } from '@shared/types'

const DASH_IDS: (keyof Dashboards)[] = ['dashboard1', 'dashboard2', 'dashboard3', 'dashboard4']

export const normalizeDashboardComponents = (
  dashboards: Dashboards | null | undefined,
  windowRole: Screen = 'main'
) => {
  if (!dashboards) return { dashboards: [] as { id: keyof Dashboards; pos: number }[] }

  const enabled = DASH_IDS.flatMap((id) => {
    const slot = dashboards[id]
    if (!slot || slot[windowRole] !== true) return []
    const pos = Number.isFinite(slot.pos) ? Math.round(slot.pos) : 9999
    return [{ id, pos }]
  }).sort((a, b) => a.pos - b.pos)

  const normalized = enabled.map((d, idx) => ({ ...d, pos: idx + 1 }))

  return { dashboards: normalized }
}
