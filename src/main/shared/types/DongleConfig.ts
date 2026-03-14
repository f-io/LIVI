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

export type PhoneTypeConfig = { frameInterval: number | null }

export type DongleConfig = {
  width: number
  height: number
  fps: number
  lastPhoneWorkMode: number
  apkVer: string
  nightMode: boolean
  carName: string
  oemName: string
  hand: HandDriveType
  mediaDelay: number
  mediaSound: 0 | 1
  callQuality: 0 | 1 | 2
  dashboardMediaInfo: boolean
  dashboardVehicleInfo: boolean
  dashboardRouteInfo: boolean
  gps: boolean
  gnssGps: boolean
  gnssGlonass: boolean
  gnssGalileo: boolean
  gnssBeiDou: boolean
  autoConn: boolean
  mapsEnabled: boolean
  audioTransferMode: boolean
  wifiType: '2.4ghz' | '5ghz'
  wifiChannel: number
  micType: MicType
  phoneConfig: Partial<Record<number, PhoneTypeConfig>>
}
