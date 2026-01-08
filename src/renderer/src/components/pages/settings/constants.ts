import { ExtraConfig } from '@main/Globals'

export const MEDIA_DELAY_MIN = 300
export const MEDIA_DELAY_MAX = 2000
export const MIN_HEIGHT = 200
export const MIN_WIDTH = 300
export const MAX_WIDTH = 4096
export const MAX_HEIGHT = 2160
export const DEFAULT_WIDTH = 800
export const DEFAULT_HEIGHT = 480
export const MIN_FPS = 20
export const MAX_FPS = 60
export const DEFAULT_FPS = 60

export const UI_DEBOUNCED_KEYS = new Set<keyof ExtraConfig>([
  'primaryColorDark',
  'primaryColorLight',
  'highlightColorDark',
  'highlightColorLight'
])

export const PRIMARY_KEYS = ['primaryColorDark', 'primaryColorLight'] as const
export const EDITABLE_KEYS = ['highlightColorDark', 'highlightColorLight'] as const
export const CAR_NAME_MAX = 20
export const OEM_LABEL_MAX = 13

export enum WiFiValues {
  '2.4ghz' = '2.4ghz',
  '5ghz' = '5ghz'
}

export enum MicTypeValues {
  'box' = 'box',
  'os' = 'os'
}

export const requiresRestartParams: (keyof ExtraConfig)[] = [
  'width',
  'height',
  'fps',
  'dpi',
  'format',
  'mediaDelay',
  'wifiType',
  'audioTransferMode',
  'carName',
  'oemName',
  'mediaSound',
  'autoPlay'
]
