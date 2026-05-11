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

export type DongleConfig = {
  aa: boolean
  cp: boolean
  wifiPassword: string
  btAdapter: string
  wifiInterface: string
  width: number
  height: number
  fps: number
  dpi: number
  projectionSafeAreaTop: number
  projectionSafeAreaBottom: number
  projectionSafeAreaLeft: number
  projectionSafeAreaRight: number
  projectionSafeAreaDrawOutside: boolean
  cluster?: { main?: boolean; dash?: boolean; aux?: boolean }
  clusterWidth: number
  clusterHeight: number
  clusterFps: number
  clusterDpi: number
  clusterSafeAreaTop: number
  clusterSafeAreaBottom: number
  clusterSafeAreaLeft: number
  clusterSafeAreaRight: number
  clusterSafeAreaDrawOutside: boolean
  lastPhoneWorkMode: PhoneWorkMode
  apkVer: string
  darkMode: boolean
  nightMode: boolean
  carName: string
  oemName: string
  hand: HandDriveType
  carType?: CarType
  evConnectorTypes?: EvConnectorType[]
  mediaDelay: number
  mediaSound: 0 | 1
  callQuality: 0 | 1 | 2
  UseBTPhone: boolean
  dashboardMediaInfo: boolean
  dashboardVehicleInfo: boolean
  dashboardRouteInfo: boolean
  gps: boolean
  gnssGps: boolean
  gnssGlonass: boolean
  gnssGalileo: boolean
  gnssBeiDou: boolean
  autoConn: boolean
  disableAudioOutput: boolean
  hwAcceleration: boolean
  wifiType: '2.4ghz' | '5ghz'
  wifiChannel: number
  micType: MicType
  phoneConfig: Partial<Record<number, PhoneTypeConfig>>
}
