import {
  General,
  Video,
  KeyBindings,
  Advanced,
  Other,
  Sources,
  ViewMode
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
} from '../../components/tabs/neewSettings/componetns'
import { RouteProps, RoutePath } from '../types'

export const settingsNestedPath = {
  general: `/${RoutePath.NewSettings}/general`,
  screen: `/${RoutePath.NewSettings}/screen`,
  sources: `/${RoutePath.NewSettings}/sources`,
  keybindings: `/${RoutePath.NewSettings}/keybindings`,
  other: `/${RoutePath.NewSettings}/other`
}

export const settingsSubNestedPath = {
  advanced: `${settingsNestedPath.other}/advanced`,
  viewMode: `${settingsNestedPath.other}/viewMode`
}

export const settingsSubNestedRoutes: RouteProps[] = [
  {
    path: settingsSubNestedPath.advanced,
    component: Advanced,
    title: 'Advanced'
  },
  {
    path: settingsSubNestedPath.viewMode,
    component: ViewMode,
    title: 'ViewMode'
  }
]

export const settingsNestedRoutes: RouteProps[] = [
  {
    path: settingsNestedPath.general,
    component: General,
    title: 'General'
  },
  {
    path: settingsNestedPath.screen,
    component: Video,
    title: 'Video & Audio'
  },
  {
    path: settingsNestedPath.sources,
    component: Sources,
    title: 'Sources'
  },
  {
    path: settingsNestedPath.keybindings,
    component: KeyBindings,
    title: 'KeyBindings'
  },
  {
    path: settingsNestedPath.other,
    component: Other,
    title: 'Other'
  },
  ...settingsSubNestedRoutes
]
