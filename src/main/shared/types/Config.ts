export enum HandDriveType {
  LHD = 0,
  RHD = 1
}

export enum MicType {
  CarMic = 0,
  DongleMic = 1,
  PhoneMic = 2
}

export enum PhoneWorkMode {
  CarPlay = 2,
  Android = 4
}

export enum PhoneType {
  AndroidMirror = 1,
  CarPlay = 3,
  iPhoneMirror = 4,
  AndroidAuto = 5,
  HiCar = 6
}

export enum CarType {
  Unknown = 0,
  Gasoline = 1,
  DieselWinter = 3, // US DIESEL_1 — low-temp / kerosene-blend diesel
  Diesel = 4, // US DIESEL_2 — regular pump diesel
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

export type PhoneTypeConfig = { frameInterval: number | null }

export type ConnectionPreference = 'auto' | 'dongle' | 'native'

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

export type WindowBounds = {
  x: number
  y: number
  width: number
  height: number
}

export type Config = {
  // Backend selection
  aa: boolean
  cp: boolean
  connectionPreference: ConnectionPreference

  // Wi-Fi + Bluetooth
  wifiPassword: string
  btAdapter: string
  wifiInterface: string
  wifiType: '2.4ghz' | '5ghz'
  wifiChannel: number

  // Main projection resolution + safe area
  width: number
  height: number
  fps: number
  dpi: number
  projectionSafeAreaTop: number
  projectionSafeAreaBottom: number
  projectionSafeAreaLeft: number
  projectionSafeAreaRight: number
  projectionSafeAreaDrawOutside: boolean

  // Cluster window
  cluster?: WindowAssignment
  clusterWidth: number
  clusterHeight: number
  clusterFps: number
  clusterDpi: number
  clusterSafeAreaTop: number
  clusterSafeAreaBottom: number
  clusterSafeAreaLeft: number
  clusterSafeAreaRight: number
  clusterSafeAreaDrawOutside: boolean

  // Phone session state
  lastPhoneWorkMode: PhoneWorkMode
  lastConnectedAaBtMac?: string
  phoneConfig: Partial<Record<number, PhoneTypeConfig>>

  apkVer: string

  // Theme / vehicle identity
  darkMode: boolean
  nightMode: boolean
  carName: string
  oemName: string
  hand: HandDriveType
  carType?: CarType
  evConnectorTypes?: EvConnectorType[]

  // Audio
  mediaDelay: number
  samplingFrequency: 0 | 1
  callQuality: 0 | 1 | 2
  UseBTPhone: boolean
  micType: MicType
  disableAudioOutput: boolean
  audioVolume: number
  navVolume: number
  voiceAssistantVolume: number
  callVolume: number
  audioOutputDevice?: string
  audioInputDevice?: string
  visualAudioDelayMs: number

  // Decode / video
  hwAcceleration: boolean

  // Dashboard widgets
  dashboardMediaInfo: boolean
  dashboardVehicleInfo: boolean
  dashboardRouteInfo: boolean

  // GNSS forwarding
  gps: boolean
  gnssGps: boolean
  gnssGlonass: boolean
  gnssGalileo: boolean
  gnssBeiDou: boolean

  // Auto-connect + auto-switch
  autoConn: boolean
  autoSwitchOnStream: boolean
  autoSwitchOnPhoneCall: boolean
  autoSwitchOnGuidance: boolean
  autoSwitchOnReverse: boolean

  // LIVI UI
  startPage: 'home' | 'media' | 'maps' | 'telemetry' | 'camera' | 'settings'
  language: string
  kiosk: WindowAssignment
  uiZoomPercent: number
  appearanceMode: AppearanceMode

  // Camera + dashboards + media slots
  cameraId: string
  camera: WindowAssignment
  cameraMirror: boolean
  media: WindowAssignment
  dashboards: DashboardsConfig

  // Multi-window bounds
  mainScreenBounds?: WindowBounds
  dashScreenBounds?: WindowBounds
  auxScreenBounds?: WindowBounds
  dashScreenActive: boolean
  dashScreenWidth: number
  dashScreenHeight: number
  auxScreenActive: boolean
  auxScreenWidth: number
  auxScreenHeight: number

  lastKnownGps?: LastKnownGps

  dongleToolsIp?: string

  // Theme overrides
  primaryColorDark?: string
  primaryColorLight?: string
  highlightColorLight?: string
  highlightColorDark?: string

  // Dongle icon overrides
  dongleIcon120?: string
  dongleIcon180?: string
  dongleIcon256?: string

  // Key bindings
  bindings: KeyBindings
}

export type KeyBindings = {
  // D-PAD
  up: string
  down: string
  left: string
  right: string
  selectUp: string
  selectDown: string
  back: string

  // Rotary Knob
  knobLeft: string
  knobRight: string
  knobUp: string
  knobDown: string

  // Media Control
  home: string
  playPause: string
  play: string
  pause: string
  next: string
  prev: string

  // Phone
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

  // Voice
  voiceAssistant: string
  voiceAssistantRelease: string
}

export const DEFAULT_BINDINGS: KeyBindings = {
  // D-PAD
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  selectUp: '',
  selectDown: 'Enter',
  back: 'Backspace',

  // Rotary Knob
  knobLeft: '',
  knobRight: '',
  knobUp: '',
  knobDown: '',

  // Media Control
  home: 'KeyH',
  playPause: 'KeyP',
  play: '',
  pause: '',
  next: 'KeyN',
  prev: 'KeyB',

  // Phone
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

  // Voice / UI
  voiceAssistant: 'KeyV',
  voiceAssistantRelease: ''
}
