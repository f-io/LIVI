import { PAGES, ROUTES } from '@shared/types'
import type { ReactElement } from 'react'
import type { RouteObject } from 'react-router'
import { Layout } from '../components/layouts/Layout'
import { Camera, Custom, Media, Telemetry } from '../components/pages'
import { SettingsPage } from '../components/pages/settings/SettingsPage'
import { settingsRoutes } from './schemas/schema'

const elements: Partial<Record<ROUTES, ReactElement>> = {
  [ROUTES.TELEMETRY]: <Telemetry />,
  [ROUTES.MEDIA]: <Media />,
  [ROUTES.CAMERA]: <Camera />,
  [ROUTES.CUSTOM]: <Custom />
}

export const appRoutes: RouteObject[] = [
  {
    path: ROUTES.HOME,
    element: <Layout />,
    children: PAGES.flatMap(({ path }): RouteObject[] => {
      if (path === ROUTES.SETTINGS) {
        return [
          {
            path: `${path}/*`,
            element: <SettingsPage />,
            children: settingsRoutes?.children ?? []
          }
        ]
      }
      const element = elements[path]
      return element ? [{ path, element }] : []
    })
  }
]
