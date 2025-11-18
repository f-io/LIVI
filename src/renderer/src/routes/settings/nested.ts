import {
  General,
  VideoAudio,
  KeyBindings,
  Advanced,
  Other,
  Sources,
  UiTheme,
  About
} from '../../components/pages/newSettings/subPages'
import { RouteProps, RoutePath } from '../types'

export const settingsNestedPath = {
  general: `/${RoutePath.NewSettings}/general`,
  screen: `/${RoutePath.NewSettings}/screen`,
  sources: `/${RoutePath.NewSettings}/sources`,
  keybindings: `/${RoutePath.NewSettings}/keybindings`,
  other: `/${RoutePath.NewSettings}/other`,
  about: `/${RoutePath.NewSettings}/about`
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
    component: UiTheme,
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
    component: VideoAudio,
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
  {
    path: settingsNestedPath.about,
    component: About,
    title: 'About'
  },
  ...settingsSubNestedRoutes
]
