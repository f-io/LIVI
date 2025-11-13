import { Home, Media, Camera, Settings, NewSettings, Info } from '../components/tabs'
import { RoutePath, RouteProps } from './types'
import { settingsRoutes } from './settings'

export const routes: RouteProps[] = [
  {
    path: `/${RoutePath.Settings}`,
    component: Settings
  },
  ...settingsRoutes,
  {
    path: `/${RoutePath.Info}`,
    component: Info
  },
  {
    path: `/${RoutePath.Camera}`,
    component: Camera
  },
  {
    path: `/${RoutePath.Media}`,
    component: Media
  },
  {
    path: `/${RoutePath.Home}`,
    component: Home
  }
]
