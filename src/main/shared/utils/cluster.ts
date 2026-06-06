type ClusterDisplayMap = { main?: boolean; dash?: boolean; aux?: boolean }

type ClusterAwareConfig = {
  dashboards?: {
    dash3?: ClusterDisplayMap | null
    dash4?: ClusterDisplayMap | null
  } | null
}

export type ClusterScreen = 'main' | 'dash' | 'aux'

export function clusterTargetScreens(cfg: ClusterAwareConfig | null | undefined): ClusterScreen[] {
  const d3 = cfg?.dashboards?.dash3
  const d4 = cfg?.dashboards?.dash4
  const out: ClusterScreen[] = []
  for (const role of ['main', 'dash', 'aux'] as const) {
    if (d3?.[role] === true || d4?.[role] === true) out.push(role)
  }
  return out
}

export function isClusterOnScreen(
  cfg: ClusterAwareConfig | null | undefined,
  role: ClusterScreen
): boolean {
  return clusterTargetScreens(cfg).includes(role)
}

export function isClusterDisplayed(cfg: ClusterAwareConfig | null | undefined): boolean {
  return clusterTargetScreens(cfg).length > 0
}
