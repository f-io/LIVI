import type { TelemetryDashboardConfig } from '@shared/types'

export const normalizeDashComponents = (
  enabledDashboards: TelemetryDashboardConfig[] | null | undefined
) => {
  const baseDashboards = Array.isArray(enabledDashboards) ? enabledDashboards : []

  const enabled = baseDashboards
    .filter((d) => d && d.enabled)
    .map((d) => ({
      id: d.id,
      pos: Number.isFinite(d.pos) ? Math.round(d.pos) : 9999
    }))
    .sort((a, b) => a.pos - b.pos)

  const normalized = enabled.map((d, idx) => ({ ...d, pos: idx + 1 }))

  return {
    dashboards: normalized
  }
}
