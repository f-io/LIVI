import type { DashboardsConfig, TelemetryDashboardId, WindowId } from '@shared/types'

const DASH_IDS: TelemetryDashboardId[] = ['dash1', 'dash2', 'dash3', 'dash4']

export const normalizeDashComponents = (
  dashboards: DashboardsConfig | null | undefined,
  windowRole: WindowId = 'main'
) => {
  if (!dashboards) return { dashboards: [] as { id: TelemetryDashboardId; pos: number }[] }

  const enabled = DASH_IDS.flatMap((id) => {
    const slot = dashboards[id]
    if (!slot || slot[windowRole] !== true) return []
    const pos = Number.isFinite(slot.pos) ? Math.round(slot.pos) : 9999
    return [{ id, pos }]
  }).sort((a, b) => a.pos - b.pos)

  const normalized = enabled.map((d, idx) => ({ ...d, pos: idx + 1 }))

  return { dashboards: normalized }
}
