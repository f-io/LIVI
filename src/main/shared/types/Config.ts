export enum HandDriveType {
  LHD = 0,
  RHD = 1
}

export enum CarType {
  Unknown = 0,
  Gasoline = 1,
  DieselWinter = 3, // US DIESEL_1, low-temperature or kerosene-blend diesel
  Diesel = 4, // US DIESEL_2, regular pump diesel
  Biodiesel = 5,
  E85 = 6,
  LPG = 7,
  CNG = 8,
  LNG = 9,
  Electric = 10,
  HybridGasoline = 101,
  HybridDiesel = 102,
  Hydrogen = 11,
  Other = 12
}

export enum EvConnectorType {
  Unknown = 0,
  J1772 = 1,
  Mennekes = 2,
  Chademo = 3,
  Combo1 = 4,
  Combo2 = 5,
  TeslaSupercharger = 8,
  Gbt = 9,
  Other = 101
}

export type TelemetryDashboardId = 'dash1' | 'dash2' | 'dash3' | 'dash4'

export type WindowId = 'main' | 'dash' | 'aux'

export type WindowAssignment = {
  main: boolean
  dash: boolean
  aux: boolean
}

export type DashboardSlotConfig = WindowAssignment & {
  pos: number
}

export type DashboardsConfig = Record<TelemetryDashboardId, DashboardSlotConfig>

export type LastKnownGps = {
  lat: number
  lng: number
  alt?: number
  heading?: number
  ts: number
}

export type AppearanceMode = 'auto' | 'night' | 'day'

export type SpeedUnit = 'kmh' | 'mph'

export type TemperatureUnit = 'celsius' | 'fahrenheit'

/** WPA passphrase bounds. */
export const WIFI_PASSWORD_MIN = 8
export const WIFI_PASSWORD_MAX = 63

export type WindowBounds = {
  x: number
  y: number
  width: number
  height: number
}

export type Config = {
  debugLogging: boolean

  wirelessAaEnabled: boolean
  wirelessCpEnabled: boolean

  wifiPassword: string
  btAdapter: string
  wifiInterface: string
  wifiDedicatedInterface: boolean
  wifiType: '2.4ghz' | '5ghz' | '6ghz'
  wifiChannel: number
  wifiChannelWidth: number
  country: string

  // AirPlay protocol version we advertise
  carPlaySourceVersion: string

  // CarPlay MFi coprocessor: i2c bus, power-enable GPIO (-1 = no power pin)
  carPlayMfiI2cBus: number
  carPlayMfiPowerGpio: number

  gpsEnabled: boolean
  gpsDevice: string
  gpsBaudRate: number

  /** Last zone derived from a fix, applied at startup. */
  timezone: string

  projectionWidth: number
  projectionHeight: number
  projectionFps: number
  projectionDpi: number
  // View Area = the rendered stream region
  projectionViewAreaTop: number
  projectionViewAreaBottom: number
  projectionViewAreaLeft: number
  projectionViewAreaRight: number
  // Safe Area = where the phone keeps nav hints/UI inside the view area.
  projectionSafeAreaTop: number
  projectionSafeAreaBottom: number
  projectionSafeAreaLeft: number
  projectionSafeAreaRight: number
  projectionSafeAreaDrawOutside: boolean

  clusterWidth: number
  clusterHeight: number
  clusterFps: number
  clusterDpi: number
  // View Area = the rendered stream region
  clusterViewAreaTop: number
  clusterViewAreaBottom: number
  clusterViewAreaLeft: number
  clusterViewAreaRight: number
  // Safe Area = where the phone keeps nav hints/UI inside the view area
  clusterSafeAreaTop: number
  clusterSafeAreaBottom: number
  clusterSafeAreaLeft: number
  clusterSafeAreaRight: number

  lastConnectedAaBtMac?: string

  darkMode: boolean
  overlayMessages: boolean
  displayBrightness: number
  displayBrightnessAuto: boolean
  carName: string
  oemName: string
  hand: HandDriveType
  carType?: CarType
  evConnectorTypes?: EvConnectorType[]
  maxSpeedKph: number
  /** Speed between two numbered marks on the dash, in the unit shown. */
  speedScaleStep: number
  maxRpm: number
  /** 0 for none. */
  redlineRpm: number
  speedUnit: SpeedUnit
  temperatureUnit: TemperatureUnit

  samplingFrequency: 0 | 1
  disableAudioOutput: boolean
  huVolume: number
  huVolumeLinkSystem: boolean
  audioVolume: number
  navVolume: number
  voiceAssistantVolume: number
  callVolume: number
  systemSoundsVolume?: number
  audioOutputDevice?: string
  audioOutputDeviceLabel?: string
  audioInputDevice?: string
  audioInputDeviceLabel?: string
  visualAudioDelayMs: number

  autoConn: boolean
  autoSwitchOnReverse: boolean

  startPage: string
  language: string
  kiosk: WindowAssignment
  uiZoomPercent: number
  appearanceMode: AppearanceMode

  // Panel mode as "WIDTHxHEIGHT", empty leaves the display at the mode it came up in
  displayMode: string

  // Display calibration, applied as the Pi compositor output gamma LUT
  displayGamma: number
  displayContrast: number
  displayColorR: number
  displayColorG: number
  displayColorB: number

  cameraId: string
  camera: WindowAssignment
  cameraMirror: boolean
  cameraRotation: 0 | 90 | 180 | 270
  media: WindowAssignment
  dashboards: DashboardsConfig
  custom: WindowAssignment
  customUrl: string

  mainScreenBounds?: WindowBounds
  dashScreenBounds?: WindowBounds
  auxScreenBounds?: WindowBounds
  mainScreenWidth: number
  mainScreenHeight: number
  dashScreenActive: boolean
  dashScreenWidth: number
  dashScreenHeight: number
  auxScreenActive: boolean
  auxScreenWidth: number
  auxScreenHeight: number

  lastKnownGps?: LastKnownGps

  primaryColorDark?: string
  primaryColorLight?: string
  highlightColorLight?: string
  highlightColorDark?: string
  backgroundColorDark?: string
  backgroundColorLight?: string

  // Overrides for the logo CarPlay shows on the tile that leads back to LIVI
  carplayIcon120?: string
  carplayIcon180?: string
  carplayIcon256?: string

  // On follows the rolling build of main, off the latest release
  updateNightly: boolean

  // System packages the user chose not to be asked about again
  dismissedPackages: string[]

  bindings: KeyBindings
}

export type KeyBindings = {
  up: string
  down: string
  left: string
  right: string
  selectUp: string
  selectDown: string
  back: string

  knobLeft: string
  knobRight: string
  knobUp: string
  knobDown: string

  home: string
  cycleSession: string
  playPause: string
  play: string
  pause: string
  next: string
  prev: string

  acceptPhone: string
  rejectPhone: string
  phoneKey0: string
  phoneKey1: string
  phoneKey2: string
  phoneKey3: string
  phoneKey4: string
  phoneKey5: string
  phoneKey6: string
  phoneKey7: string
  phoneKey8: string
  phoneKey9: string
  phoneKeyStar: string
  phoneKeyHash: string
  phoneKeyHookSwitch: string

  voiceAssistant: string
  voiceAssistantRelease: string
}

export const DEFAULT_BINDINGS: KeyBindings = {
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  selectUp: '',
  selectDown: 'Enter',
  back: 'Backspace',

  knobLeft: '',
  knobRight: '',
  knobUp: '',
  knobDown: '',

  home: 'KeyH',
  cycleSession: 'KeyS',
  playPause: 'KeyP',
  play: '',
  pause: '',
  next: 'KeyN',
  prev: 'KeyB',

  acceptPhone: 'KeyA',
  rejectPhone: 'KeyR',
  phoneKey0: 'Digit0',
  phoneKey1: 'Digit1',
  phoneKey2: 'Digit2',
  phoneKey3: 'Digit3',
  phoneKey4: 'Digit4',
  phoneKey5: 'Digit5',
  phoneKey6: 'Digit6',
  phoneKey7: 'Digit7',
  phoneKey8: 'Digit8',
  phoneKey9: 'Digit9',
  phoneKeyStar: '',
  phoneKeyHash: '',
  phoneKeyHookSwitch: '',

  voiceAssistant: 'KeyV',
  voiceAssistantRelease: ''
}
