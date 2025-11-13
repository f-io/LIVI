import { RoutePath, RouteProps } from '../types'
import { settingsNestedRoutes } from './nested'
import { NewSettings } from '../../components/tabs'

export const settingsRoutes: RouteProps[] = [
  {
    path: `/${RoutePath.NewSettings}`,
    component: NewSettings
  },
  ...settingsNestedRoutes
]
