import type { Config } from '@shared/types'
import { RouteObject } from 'react-router'
import { SettingsPage } from '../components/pages/settings/SettingsPage'
import { SettingsNode } from '../routes'

export const generateRoutes = (node: SettingsNode<Config>): RouteObject | null => {
  if (node.type !== 'route') {
    return node.page && node.path ? { path: node.path, element: <SettingsPage /> } : null
  }

  return {
    path: node.route,
    element: <SettingsPage />,
    children: node.children?.map(generateRoutes).filter((r): r is RouteObject => !!r)
  }
}
