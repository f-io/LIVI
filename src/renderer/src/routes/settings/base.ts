import { RoutePath, RouteProps } from '../types'
import { settingsNestedRoutes } from './nested'
// TODO Required for new settings UI
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
import { NewSettings } from '../../components/tabs'

export const settingsRoutes: RouteProps[] = [
  {
    path: `/${RoutePath.NewSettings}`,
    component: NewSettings
  },
  ...settingsNestedRoutes
]
