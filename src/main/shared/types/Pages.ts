export enum ROUTES {
  HOME = '/',
  TELEMETRY = '/telemetry',
  MEDIA = '/media',
  CAMERA = '/camera',
  CUSTOM = '/custom',
  SETTINGS = '/settings'
}

export type Page = {
  path: ROUTES
  label: string
  labelKey: string
}

/** Top-level pages in navigation order. */
export const PAGES: readonly Page[] = [
  { path: ROUTES.HOME, label: 'Home', labelKey: 'settings.startPageHome' },
  { path: ROUTES.TELEMETRY, label: 'Telemetry', labelKey: 'settings.startPageTelemetry' },
  { path: ROUTES.MEDIA, label: 'Media', labelKey: 'settings.startPageMedia' },
  { path: ROUTES.CAMERA, label: 'Camera', labelKey: 'settings.startPageCamera' },
  { path: ROUTES.CUSTOM, label: 'Custom', labelKey: 'settings.startPageCustom' },
  { path: ROUTES.SETTINGS, label: 'Settings', labelKey: 'settings.startPageSettings' }
]

export function isPagePath(path: unknown): path is ROUTES {
  return PAGES.some((p) => p.path === path)
}
