import { ForwardRefExoticComponent, RefAttributes, SVGProps } from 'react'

export enum ROUTES_NEW {
  HOME = '/',
  MEDIA = '/media',
  CAMERA = '/camera',
  INFO = '/info',
  SETTINGS = '/settings',
  NEW_SETTINGS = '/new-settings',
  QUIT = 'quit'
}

export enum RoutePath {
  Home = 'home',
  Settings = 'settings',
  NewSettings = 'new-settings',
  Camera = 'camera',
  Media = 'media',
  Info = 'info'
}

export interface RouteProps {
  path: string
  component?: unknown // TODO fix this
  icon?: ForwardRefExoticComponent<
    Omit<SVGProps<SVGSVGElement>, 'ref'> & {
      title?: string | undefined
      titleId?: string | undefined
    } & RefAttributes<SVGSVGElement>
  >
  title?: string
}
