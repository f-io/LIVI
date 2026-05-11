type ClusterDisplayMap = { main?: boolean; dash?: boolean; aux?: boolean }

type ClusterAwareConfig = {
  cluster?: ClusterDisplayMap | null
}

export function isClusterDisplayed(cfg: ClusterAwareConfig | null | undefined): boolean {
  const c = cfg?.cluster
  if (!c) return false
  return c.main === true || c.dash === true || c.aux === true
}
