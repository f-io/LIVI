export enum ROUTES {
  HOME = '/',
  MEDIA = '/media',
  CAMERA = '/camera',
  SETTINGS = '/settings',
  QUIT = 'quit'
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

export const EMPTY_STRING = '—'

export const UI = {
  MIN_HEIGHT_SHOW_TIME_WIFI: 320,
  XS_ICON_MAX_HEIGHT: 320
} as const
