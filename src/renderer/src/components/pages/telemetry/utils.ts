export const normalizeDashComponents = (enabledDashboards) => {
  const baseDashboards = Array.isArray(enabledDashboards) ? enabledDashboards : []

  const enabled = baseDashboards
    .filter((d) => d && Boolean(d.enabled))
    .map((d) => ({
      id: d.id,
      pos: Number.isFinite(d.pos) ? Math.round(d.pos) : 9999
    }))
    .sort((a, b) => a.pos - b.pos)

  // stable normalize positions
  const normalized = enabled.map((d, idx) => ({ ...d, pos: idx + 1 }))

  return {
    dashboards: normalized
  }
}
