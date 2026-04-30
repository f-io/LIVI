export type DevListEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
  source?: 'dongle' | 'host'
}

export type BoxInfoPayload = {
  uuid?: string
  MFD?: string
  boxType?: string
  OemName?: string
  productType?: string
  HiCar?: number
  supportLinkType?: string
  supportFeatures?: string
  hwVersion?: string
  wifiChannel?: number
  CusCode?: string
  DevList?: DevListEntry[]
  ChannelList?: string
  MDLinkType?: string
  MDModel?: string
  MDOSVersion?: string
  MDLinkVersion?: string
  btMacAddr?: string
  btName?: string
  cpuTemp?: number
}

export type DongleFirmwareAction = 'check' | 'download' | 'upload' | 'status'

export type DongleFwApiRaw = {
  err: number
  token?: string
  ver?: string
  size?: string | number
  id?: string
  notes?: string
  msg?: string
  error?: string
}
