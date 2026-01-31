import { RoutePath } from './types'
import { Home, Media, Camera, Maps } from '../components/pages'
import { settingsRoutes } from './schemas.ts/schema'
import { Layout } from '../components/layouts/Layout'
import { SettingsPage } from '../components/pages/settings/SettingsPage'

export const appRoutes = [
  {
    path: '/',
    element: <Layout />,
    children: [
      {
        path: `/${RoutePath.Home}`,
        element: <Home />
      },
      {
        path: `/${RoutePath.Maps}`,
        element: <Maps />
      },
      {
        path: `/${RoutePath.Media}`,
        element: <Media />
      },
      {
        path: `/${RoutePath.Camera}`,
        element: <Camera />
      },
      {
        path: `/${RoutePath.Settings}/*`,
        element: <SettingsPage />,
        children: settingsRoutes?.children ?? []
      }
    ]
  }
]
