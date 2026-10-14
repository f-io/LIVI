import type { Config } from '@shared/types'

export const MIN_HEIGHT = 200
export const MIN_WIDTH = 300
export const MAX_WIDTH = 4096
export const MAX_HEIGHT = 2160
export const DEFAULT_WIDTH = 800
export const DEFAULT_HEIGHT = 480

export const MIN_FPS = 20
export const MAX_FPS = 60

export const MIN_DPI = 0
export const MAX_DPI = 640

export const SAFE_AREA_MIN = 0
export const SAFE_AREA_MAX_WIDTH = MAX_WIDTH
export const SAFE_AREA_MAX_HEIGHT = MAX_HEIGHT
export const AREA_STEP = 2

export const requiresRestartParams: (keyof Config)[] = [
  'wirelessAaEnabled',
  'wirelessCpEnabled',
  'carPlayHlsVideo',
  'carPlayVehicleClimate',
  'carPlayDockPosition',

  'displayMode',

  'projectionWidth',
  'projectionHeight',
  'projectionFps',
  'projectionDpi',
  'projectionViewAreaTop',
  'projectionViewAreaBottom',
  'projectionViewAreaLeft',
  'projectionViewAreaRight',
  'projectionSafeAreaTop',
  'projectionSafeAreaBottom',
  'projectionSafeAreaLeft',
  'projectionSafeAreaRight',
  'projectionSafeAreaDrawOutside',

  'clusterWidth',
  'clusterHeight',
  'clusterFps',
  'clusterDpi',
  'clusterViewAreaTop',
  'clusterViewAreaBottom',
  'clusterViewAreaLeft',
  'clusterViewAreaRight',
  'clusterSafeAreaTop',
  'clusterSafeAreaBottom',
  'clusterSafeAreaLeft',
  'clusterSafeAreaRight',

  'wifiType',
  'wifiChannel',
  'wifiChannelWidth',
  'country',
  'wifiInterface',
  'btAdapter',
  'wifiDedicatedInterface',
  'disableAudioOutput',
  'autoConn',
  'carName',
  'oemName',
  'wifiPassword',
  'samplingFrequency',
  'hand',
  'carType',
  'carplayIcon120',
  'carplayIcon180',
  'carplayIcon256'
]
