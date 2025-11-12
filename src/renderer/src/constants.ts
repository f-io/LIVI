export enum ROUTES {
  HOME = '/',
  MEDIA = '/media',
  CAMERA = '/camera',
  INFO = '/info',
  SETTINGS = '/settings',
  NEW_SETTINGS = '/new-settings',
  QUIT = 'quit'
}

export const indexToRoute: Record<number, string | 'quit'> = {
  0: ROUTES.HOME,
  1: ROUTES.MEDIA,
  2: ROUTES.CAMERA,
  3: ROUTES.INFO,
  4: ROUTES.SETTINGS,
  5: ROUTES.NEW_SETTINGS,
  6: ROUTES.QUIT
}

export const routeToIndex: Record<string, number> = {
  [ROUTES.HOME]: 0,
  [ROUTES.MEDIA]: 1,
  [ROUTES.CAMERA]: 2,
  [ROUTES.INFO]: 3,
  [ROUTES.SETTINGS]: 4,
  [ROUTES.NEW_SETTINGS]: 5,
  [ROUTES.QUIT]: 6
}

export const FOCUSABLE_SELECTOR = [
  'button:not([disabled])',
  'a[href]',
  '[role="button"]:not([aria-disabled="true"])',
  '[role="tab"]',
  '[role="menuitem"]',
  '[role="treeitem"]',
  '[role="slider"]',
  '[role="spinbutton"]',
  '[role="switch"]',
  'input:not([disabled]):not([type="hidden"])',
  'input[type="checkbox"]:not([disabled])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  '[tabindex]:not([tabindex="-1"])'
].join(',')

export enum THEME {
  LIGHT = 'light',
  DARK = 'dark'
}
