import { Home, Media, Camera, Settings, NewSettings, Info } from '../components/tabs'
import { RoutePath, RouteProps } from './types'

export const routes: RouteProps[] = [
  {
    path: `/${RoutePath.Settings}`,
    component: Settings
  },
  {
    path: `/${RoutePath.NewSettings}`,
    component: NewSettings
  },
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
