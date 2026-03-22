import type { ExtraConfig } from '@shared/types'

export const MEDIA_DELAY_MIN = 300
export const MEDIA_DELAY_MAX = 2000
export const MEDIA_DELAY_STEP = 50

export const MIN_HEIGHT = 200
export const MIN_WIDTH = 300
export const MAX_WIDTH = 4096
export const MAX_HEIGHT = 2160
export const DEFAULT_WIDTH = 800
export const DEFAULT_HEIGHT = 480

export const MIN_FPS = 20
export const MAX_FPS = 60
export const DEFAULT_FPS = 60

export const SAFE_AREA_MIN = 0
export const SAFE_AREA_MAX_WIDTH = MAX_WIDTH
export const SAFE_AREA_MAX_HEIGHT = MAX_HEIGHT
export const SAFE_AREA_DEFAULT = 0

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

export const requiresRestartParams: (keyof ExtraConfig)[] = [
  'width',
  'height',
  'fps',
  'projectionSafeAreaTop',
  'projectionSafeAreaBottom',
  'projectionSafeAreaLeft',
  'projectionSafeAreaRight',
  'projectionSafeAreaDrawOutside',

  'naviWidth',
  'naviHeight',
  'naviFps',
  'naviSafeAreaTop',
  'naviSafeAreaBottom',
  'naviSafeAreaLeft',
  'naviSafeAreaRight',
  'naviSafeAreaDrawOutside',

  'mediaDelay',
  'wifiType',
  'audioTransferMode',
  'callQuality',
  'dashboardMediaInfo',
  'dashboardVehicleInfo',
  'dashboardRouteInfo',
  'micType',
  'gps',
  'gnssGps',
  'gnssGlonass',
  'gnssGalileo',
  'gnssBeiDou',
  'autoConn',
  'carName',
  'oemName',
  'mediaSound',
  'hand',
  'mapsEnabled'
]
