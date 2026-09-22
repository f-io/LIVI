import type { PlistValue } from '../bplist'
import { ALT_UUID, buildInfoPlist, MAIN_UUID } from '../getInfo'
import { KNOB_HID_UID, MEDIA_HID_UID, TELEPHONY_HID_UID, TOUCH_HID_UID } from '../hid'
import type { CpStackConfig } from '../types'

type Dict = { [key: string]: PlistValue }

const CARPLAY_FEATURES = 0x615653aee2
const NO_AUDIO_FEATURES = Number(BigInt(CARPLAY_FEATURES) & ~BigInt(0x10004540a00))

function baseConfig(overrides: Partial<CpStackConfig> = {}): CpStackConfig {
  return {
    deviceName: 'LIVI Test',
    deviceId: 'aa:bb:cc:dd:ee:ff',
    btMac: '11:22:33:44:55:66',
    sourceVersion: '320.17',
    hevc: false,
    h264: true,
    main: { widthPixels: 800, heightPixels: 480 },
    port: 7000,
    entertainmentSampleRate: 44100,
    mfi: {} as CpStackConfig['mfi'],
    oemLabel: 'LIVI',
    icons: [],
    rightHandDrive: false,
    ...overrides
  }
}

describe('buildInfoPlist', () => {
  test('minimal config declares identity, full features and display defaults', () => {
    const info = buildInfoPlist(baseConfig()) as Dict
    expect(info.sourceVersion).toBe('320.17')
    expect(info.features).toBe(CARPLAY_FEATURES)
    expect(info.statusFlags).toBe(4)
    expect(info.model).toBe('LIVI')
    expect(info.deviceID).toBe('aa:bb:cc:dd:ee:ff')
    expect(info.bluetoothIDs).toEqual(['11:22:33:44:55:66'])
    expect(info.name).toBe('LIVI Test')
    expect(info.extendedFeatures).toEqual(['vocoderInfo', 'enhancedRequestCarUI'])
    expect(info.oemIconVisible).toBeUndefined()
    expect(info.oemIcons).toBeUndefined()
    expect(info.hevcInfo).toBeUndefined()

    const displays = info.displays as Dict[]
    expect(displays.length).toBe(1)
    const main = displays[0]
    expect(main.uuid).toBe(MAIN_UUID)
    expect(main.type).toBe(110)
    expect(main.maxFPS).toBe(60)
    expect(main.widthPixels).toBe(800)
    expect(main.heightPixels).toBe(480)
    expect(main.widthPhysical).toBe(200)
    expect(main.heightPhysical).toBe(120)
    expect(main.primaryInputDevice).toBe(3)
    expect(main.viewAreas).toBeUndefined()
    expect(main.initialViewArea).toBeUndefined()
    expect(main.initialURL).toBeUndefined()

    const hid = info.hidDevices as Dict[]
    expect(hid.map((h) => h.uuid)).toEqual(
      [TOUCH_HID_UID, KNOB_HID_UID, MEDIA_HID_UID, TELEPHONY_HID_UID].map((u) => u.toString(16))
    )
  })

  test('advertises 44.1k audio formats and latencies when audio is enabled', () => {
    const info = buildInfoPlist(baseConfig()) as Dict
    const latencies = info.audioLatencies as Dict[]
    expect(latencies.length).toBe(9)
    expect(latencies[0]).toEqual({ type: 100, inputLatencyMicros: 0, outputLatencyMicros: 0 })
    expect(latencies[1].audioType).toBe('default')

    const formats = info.audioFormats as Dict[]
    expect(formats.length).toBe(9)
    const compat = formats[0]
    expect(compat.type).toBe(100)
    expect(compat.audioType).toBe('compatibility')
    expect(compat.audioOutputFormats).toBe(0x3fc | 0xc00)
    expect(compat.audioInputFormats).toBe(0x154 | 0x400)
    const mainAudio = formats[1]
    expect(mainAudio.audioInputFormats).toBeUndefined()
    const entertainment = formats[8]
    expect(entertainment.type).toBe(102)
    expect(entertainment.audioOutputFormats).toBe(0x400000)
  })

  test('advertises 48k variants when entertainmentSampleRate is 48000', () => {
    const info = buildInfoPlist(baseConfig({ entertainmentSampleRate: 48000 })) as Dict
    const formats = info.audioFormats as Dict[]
    expect(formats[0].audioOutputFormats).toBe(0x3fc | 0xc000)
    expect(formats[0].audioInputFormats).toBe(0x154 | 0x4000)
    expect((formats[8] as Dict).audioOutputFormats).toBe(0x800000)
  })

  test('disableAudioOutput clears audio feature bits and omits audio keys', () => {
    const info = buildInfoPlist(baseConfig({ disableAudioOutput: true })) as Dict
    expect(info.features).toBe(NO_AUDIO_FEATURES)
    expect(info.audioLatencies).toBeUndefined()
    expect(info.audioFormats).toBeUndefined()
    expect(info.modes).toBeDefined()
  })

  test('announces the driver side', () => {
    expect((buildInfoPlist(baseConfig({ rightHandDrive: false })) as Dict).rightHandDrive).toBe(
      false
    )
    expect((buildInfoPlist(baseConfig({ rightHandDrive: true })) as Dict).rightHandDrive).toBe(true)
  })

  test('modes declares screen and audio resources plus app states', () => {
    const info = buildInfoPlist(baseConfig()) as Dict
    const modes = info.modes as Dict
    const resources = modes.resources as Dict[]
    expect(resources.map((r) => r.resourceID)).toEqual([1, 2])
    expect(resources[0].transferType).toBe(1)
    expect(resources[0].transferPriority).toBe(100)
    expect(resources[0].takeConstraint).toBe(100)
    const appStates = modes.appStates as Dict[]
    expect(appStates).toEqual([
      { appStateID: 2, state: false },
      { appStateID: 1, speechMode: -1 },
      { appStateID: 3, state: false }
    ])
  })

  test('viewArea without safeArea yields a viewAreas entry without safeArea', () => {
    const cfg = baseConfig({
      main: {
        widthPixels: 800,
        heightPixels: 480,
        viewArea: { top: 10, bottom: 20, left: 30, right: 40 }
      }
    })
    const info = buildInfoPlist(cfg) as Dict
    const main = (info.displays as Dict[])[0]
    const views = main.viewAreas as Dict[]
    expect(main.initialViewArea).toBe(0)
    expect(views.length).toBe(1)
    expect(views[0]).toEqual({
      widthPixels: 730,
      heightPixels: 450,
      originXPixels: 30,
      originYPixels: 10
    })
  })

  test('full config covers cluster, safe areas, physical size, icons and hevc', () => {
    const cfg = baseConfig({
      hevc: true,
      main: {
        widthPixels: 1920,
        heightPixels: 720,
        widthPhysicalMm: 300,
        heightPhysicalMm: 112,
        fps: 30,
        primaryInputDevice: 1,
        viewArea: { top: 0, bottom: 0, left: 100, right: 0 },
        safeArea: { top: 8, bottom: 8, left: 108, right: 8 },
        safeAreaDrawOutside: false,
        initialUrl: 'carplay://main'
      },
      cluster: {
        widthPixels: 480,
        heightPixels: 272,
        viewArea: { top: 0, bottom: 0, left: 0, right: 0 },
        safeArea: { top: 4, bottom: 4, left: 4, right: 4 },
        initialUrl: 'carplay://cluster'
      },
      icons: [{ widthPixels: 120, heightPixels: 120, data: Buffer.from([0x89, 0x50]) }]
    })
    const info = buildInfoPlist(cfg) as Dict
    expect(info.hevcInfo).toEqual({})
    expect(info.oemIconVisible).toBe(true)
    expect(info.oemIconLabel).toBe('LIVI')
    const icons = info.oemIcons as Dict[]
    expect(icons.length).toBe(1)
    expect((icons[0].imageData as Buffer).equals(Buffer.from([0x89, 0x50]))).toBe(true)
    expect(icons[0].widthPixels).toBe(120)
    expect(icons[0].heightPixels).toBe(120)
    expect(icons[0].prerendered).toBe(true)

    const displays = info.displays as Dict[]
    expect(displays.length).toBe(2)
    const main = displays[0]
    expect(main.maxFPS).toBe(30)
    expect(main.widthPhysical).toBe(300)
    expect(main.heightPhysical).toBe(112)
    expect(main.primaryInputDevice).toBe(1)
    expect(main.initialURL).toBe('carplay://main')
    const mainView = (main.viewAreas as Dict[])[0]
    expect(mainView.widthPixels).toBe(1820)
    expect(mainView.originXPixels).toBe(100)
    const mainSafe = mainView.safeArea as Dict
    expect(mainSafe.widthPixels).toBe(1920 - 108 - 8)
    expect(mainSafe.heightPixels).toBe(720 - 16)
    expect(mainSafe.originXPixels).toBe(108)
    expect(mainSafe.originYPixels).toBe(8)
    expect(mainSafe.drawUIOutsideSafeArea).toBe(false)

    const cluster = displays[1]
    expect(cluster.uuid).toBe(ALT_UUID)
    expect(cluster.type).toBe(111)
    expect(cluster.initialURL).toBe('carplay://cluster')
    const clusterView = (cluster.viewAreas as Dict[])[0]
    expect(clusterView.widthPixels).toBe(480)
    const clusterSafe = clusterView.safeArea as Dict
    expect(clusterSafe.widthPixels).toBe(472)
    expect(clusterSafe.drawUIOutsideSafeArea).toBeUndefined()
  })

  test('touch hid device is sized to the main display', () => {
    const info = buildInfoPlist(baseConfig()) as Dict
    const touch = (info.hidDevices as Dict[])[0]
    const d = touch.hidDescriptor as Buffer
    expect(d[39]).toBe(800 & 0xff)
    expect(d[40]).toBe(800 >> 8)
    expect(d[50]).toBe(480 & 0xff)
    expect(d[51]).toBe(480 >> 8)
  })
})
