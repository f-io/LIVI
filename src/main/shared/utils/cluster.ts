type ClusterDisplayMap = { main?: boolean; cluster?: boolean; aux?: boolean }

type ClusterAwareConfig = {
  dashboards?: {
    dashboard3?: ClusterDisplayMap | null
    dashboard4?: ClusterDisplayMap | null
  } | null
}

export type ClusterScreen = 'main' | 'cluster' | 'aux'

export function clusterTargetScreens(cfg: ClusterAwareConfig | null | undefined): ClusterScreen[] {
  const d3 = cfg?.dashboards?.dashboard3
  const d4 = cfg?.dashboards?.dashboard4
  const out: ClusterScreen[] = []
  for (const role of ['main', 'cluster', 'aux'] as const) {
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
