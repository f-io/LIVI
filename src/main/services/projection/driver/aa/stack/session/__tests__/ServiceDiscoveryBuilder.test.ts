import { CH, DISPLAY_TYPE, MEDIA_CODEC } from '../../constants'
import type { ProtoTypes } from '../../proto/index'
import { buildServiceDiscoveryResponse } from '../ServiceDiscoveryBuilder'
import type { SessionConfig } from '../Session'

function stubProto(): { proto: ProtoTypes; capture: { fields: Record<string, unknown> | null } } {
  const capture = { fields: null as Record<string, unknown> | null }
  const proto = {
    ServiceDiscoveryResponse: {
      create: (fields: Record<string, unknown>) => {
        capture.fields = fields
        return fields
      },
      encode: () => ({
        finish: () => new Uint8Array([0xde, 0xad, 0xbe, 0xef])
      })
    }
  } as unknown as ProtoTypes
  return { proto, capture }
}

function baseConfig(over: Partial<SessionConfig> = {}): SessionConfig {
  return {
    huName: 'LIVI',
    videoWidth: 1280,
    videoHeight: 720,
    videoDpi: 140,
    videoFps: 30,
    displayWidth: 1280,
    displayHeight: 720,
    driverPosition: 0,
    clusterEnabled: false,
    clusterWidth: 0,
    clusterHeight: 0,
    clusterFps: 0,
    clusterDpi: 0,
    ...over
  }
}

type Channel = { id: number; [k: string]: unknown }

function channelById(fields: Record<string, unknown>, id: number): Channel | undefined {
  const channels = fields.channels as Channel[]
  return channels.find((c) => c.id === id)
}

describe('buildServiceDiscoveryResponse', () => {
  test('emits the encoded buffer and the codec index map', () => {
    const { proto, capture } = stubProto()
    const r = buildServiceDiscoveryResponse(baseConfig(), proto)
    expect(r.buf).toBeInstanceOf(Buffer)
    expect(r.buf.length).toBe(4)
    expect(r.videoCodecByIndex).toEqual(['h264'])
    expect(capture.fields).not.toBeNull()
  })

  test('always advertises a single h264 main video channel by default', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig(), proto)
    const video = channelById(capture.fields!, CH.VIDEO)!
    const sink = video.mediaSinkService as { videoConfigs: object[] }
    expect(sink.videoConfigs).toHaveLength(1)
  })

  test('advertises h265 in main video when hevcSupported=true', () => {
    const { proto, capture } = stubProto()
    const r = buildServiceDiscoveryResponse(baseConfig({ hevcSupported: true }), proto)
    expect(r.videoCodecByIndex).toEqual(['h264', 'h265'])
    const video = channelById(capture.fields!, CH.VIDEO)!
    const sink = video.mediaSinkService as { videoConfigs: { videoCodecType: number }[] }
    expect(sink.videoConfigs.map((c) => c.videoCodecType)).toEqual([
      MEDIA_CODEC.VIDEO_H264_BP,
      MEDIA_CODEC.VIDEO_H265
    ])
  })

  test('does NOT advertise vp9/av1 on the main video channel', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(
      baseConfig({ hevcSupported: true, vp9Supported: true, av1Supported: true }),
      proto
    )
    const video = channelById(capture.fields!, CH.VIDEO)!
    const sink = video.mediaSinkService as { videoConfigs: { videoCodecType: number }[] }
    expect(sink.videoConfigs.map((c) => c.videoCodecType)).toEqual([
      MEDIA_CODEC.VIDEO_H264_BP,
      MEDIA_CODEC.VIDEO_H265
    ])
  })

  test('omits cluster channels when clusterEnabled=false', () => {
    const { proto, capture } = stubProto()
    const r = buildServiceDiscoveryResponse(baseConfig(), proto)
    expect(channelById(capture.fields!, CH.CLUSTER_VIDEO)).toBeUndefined()
    expect(channelById(capture.fields!, CH.CLUSTER_INPUT)).toBeUndefined()
    expect(r.clusterCodecByIndex).toEqual([])
  })

  test('includes cluster channels when clusterEnabled=true', () => {
    const { proto, capture } = stubProto()
    const r = buildServiceDiscoveryResponse(
      baseConfig({
        clusterEnabled: true,
        clusterWidth: 800,
        clusterHeight: 480,
        clusterFps: 30,
        clusterDpi: 140
      }),
      proto
    )
    const cv = channelById(capture.fields!, CH.CLUSTER_VIDEO)
    const ci = channelById(capture.fields!, CH.CLUSTER_INPUT)
    expect(cv).toBeDefined()
    expect(ci).toBeDefined()
    expect(r.clusterCodecByIndex).toEqual(['h264'])
    const sink = cv!.mediaSinkService as { displayType: number; displayId: number }
    expect(sink.displayType).toBe(DISPLAY_TYPE.CLUSTER)
    expect(sink.displayId).toBe(1)
  })

  test('cluster video advertises hevc/vp9/av1 according to caps', () => {
    const { proto, capture } = stubProto()
    const r = buildServiceDiscoveryResponse(
      baseConfig({
        clusterEnabled: true,
        clusterWidth: 800,
        clusterHeight: 480,
        clusterFps: 30,
        clusterDpi: 140,
        hevcSupported: true,
        vp9Supported: true,
        av1Supported: true
      }),
      proto
    )
    expect(r.clusterCodecByIndex).toEqual(['h264', 'h265', 'vp9', 'av1'])
    const cv = channelById(capture.fields!, CH.CLUSTER_VIDEO)!
    const sink = cv.mediaSinkService as { videoConfigs: { videoCodecType: number }[] }
    expect(sink.videoConfigs.map((c) => c.videoCodecType)).toEqual([
      MEDIA_CODEC.VIDEO_H264_BP,
      MEDIA_CODEC.VIDEO_H265,
      MEDIA_CODEC.VIDEO_VP9,
      MEDIA_CODEC.VIDEO_AV1
    ])
  })

  test('omits media + speech audio channels when disableAudioOutput=true', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig({ disableAudioOutput: true }), proto)
    expect(channelById(capture.fields!, CH.MEDIA_AUDIO)).toBeUndefined()
    expect(channelById(capture.fields!, CH.SPEECH_AUDIO)).toBeUndefined()
    // System audio + mic remain
    expect(channelById(capture.fields!, CH.SYSTEM_AUDIO)).toBeDefined()
    expect(channelById(capture.fields!, CH.MIC_INPUT)).toBeDefined()
  })

  test('always advertises bluetooth, navigation, media-info, phone-status channels', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig(), proto)
    expect(channelById(capture.fields!, CH.BLUETOOTH)).toBeDefined()
    expect(channelById(capture.fields!, CH.NAVIGATION)).toBeDefined()
    expect(channelById(capture.fields!, CH.MEDIA_INFO)).toBeDefined()
    expect(channelById(capture.fields!, CH.PHONE_STATUS)).toBeDefined()
  })

  test('includes wifi projection channel only when wifiBssid is set', () => {
    {
      const { proto, capture } = stubProto()
      buildServiceDiscoveryResponse(baseConfig(), proto)
      expect(channelById(capture.fields!, CH.WIFI)).toBeUndefined()
    }
    {
      const { proto, capture } = stubProto()
      buildServiceDiscoveryResponse(baseConfig({ wifiBssid: 'aa:bb:cc:dd:ee:ff' }), proto)
      const wifi = channelById(capture.fields!, CH.WIFI)!
      expect(wifi.wifiProjectionService).toEqual({ carWifiBssid: 'aa:bb:cc:dd:ee:ff' })
    }
  })

  test('input source touchscreen reflects video tier minus margins/insets', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(
      baseConfig({
        videoWidth: 1280,
        videoHeight: 720,
        displayWidth: 1280,
        displayHeight: 720,
        mainSafeAreaLeft: 10,
        mainSafeAreaRight: 20,
        mainSafeAreaTop: 5,
        mainSafeAreaBottom: 15
      }),
      proto
    )
    const input = channelById(capture.fields!, CH.INPUT)!
    const svc = input.inputSourceService as { touchscreen: { width: number; height: number }[] }
    expect(svc.touchscreen[0]).toEqual({ width: 1280 - 10 - 20, height: 720 - 5 - 15 })
  })

  test('aspect-ratio margins are computed when display is wider than the AA tier', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(
      baseConfig({
        // 21:9 display, but tier is 16:9 → expect horizontal margin
        videoWidth: 1280,
        videoHeight: 720,
        displayWidth: 2560,
        displayHeight: 1080
      }),
      proto
    )
    const video = channelById(capture.fields!, CH.VIDEO)!
    const cfg = (
      video.mediaSinkService as { videoConfigs: { widthMargin: number; heightMargin: number }[] }
    ).videoConfigs[0]
    expect(cfg.widthMargin).toBe(0)
    expect(cfg.heightMargin).toBeGreaterThan(0)
  })

  test('fuelTypes defaults to [1] when missing', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig(), proto)
    const sensor = channelById(capture.fields!, CH.SENSOR)!
    const svc = sensor.sensorSourceService as { supportedFuelTypes: number[] }
    expect(svc.supportedFuelTypes).toEqual([1])
  })

  test('evConnectorTypes is forwarded only when non-empty', () => {
    {
      const { proto, capture } = stubProto()
      buildServiceDiscoveryResponse(baseConfig(), proto)
      const sensor = channelById(capture.fields!, CH.SENSOR)!
      expect(
        (sensor.sensorSourceService as Record<string, unknown>).supportedEvConnectorTypes
      ).toBeUndefined()
    }
    {
      const { proto, capture } = stubProto()
      buildServiceDiscoveryResponse(baseConfig({ evConnectorTypes: [4, 5] }), proto)
      const sensor = channelById(capture.fields!, CH.SENSOR)!
      expect(
        (sensor.sensorSourceService as { supportedEvConnectorTypes: number[] })
          .supportedEvConnectorTypes
      ).toEqual([4, 5])
    }
  })

  test('top-level fields carry HU identity + driver position', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig({ huName: 'TestHU', driverPosition: 1 }), proto)
    expect(capture.fields).toMatchObject({
      displayName: 'TestHU',
      driverPosition: 1,
      probeForSupport: false
    })
  })

  test('btMacAddress defaults to zero MAC', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig(), proto)
    const bt = channelById(capture.fields!, CH.BLUETOOTH)!
    expect((bt.bluetoothService as { carAddress: string }).carAddress).toBe('00:00:00:00:00:00')
  })

  test.each([
    [800, 480, 1],
    [1280, 720, 2],
    [1920, 1080, 3],
    [2560, 1440, 4],
    [3840, 2160, 5]
  ])('main video codecResolution for %sx%s = %s', (w, h, expected) => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig({ videoWidth: w, videoHeight: h }), proto)
    const video = channelById(capture.fields!, CH.VIDEO)!
    const cfg = (video.mediaSinkService as { videoConfigs: { codecResolution: number }[] })
      .videoConfigs[0]
    expect(cfg.codecResolution).toBe(expected)
  })

  test('60 fps maps to VIDEO_FPS._60', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(baseConfig({ videoFps: 60 }), proto)
    const video = channelById(capture.fields!, CH.VIDEO)!
    const cfg = (video.mediaSinkService as { videoConfigs: { frameRate: number }[] })
      .videoConfigs[0]
    expect(cfg.frameRate).toBe(1)
  })

  test('aspect-ratio margin is on width when display is narrower than the AA tier', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(
      baseConfig({
        videoWidth: 1920,
        videoHeight: 1080,
        displayWidth: 1280,
        displayHeight: 720 + 200 // make it taller than 16:9
      }),
      proto
    )
    const video = channelById(capture.fields!, CH.VIDEO)!
    const cfg = (
      video.mediaSinkService as { videoConfigs: { widthMargin: number; heightMargin: number }[] }
    ).videoConfigs[0]
    expect(cfg.widthMargin).toBeGreaterThan(0)
    expect(cfg.heightMargin).toBe(0)
  })

  test('cluster falls back to main vRes when tier dimensions are non-standard', () => {
    const { proto, capture } = stubProto()
    buildServiceDiscoveryResponse(
      baseConfig({
        clusterEnabled: true,
        clusterWidth: 400,
        clusterHeight: 240,
        clusterTierWidth: 400,
        clusterTierHeight: 240,
        clusterFps: 0, // exercises the fallback to vFps
        clusterDpi: 100
      }),
      proto
    )
    const cv = channelById(capture.fields!, CH.CLUSTER_VIDEO)!
    const cfg = (
      cv.mediaSinkService as { videoConfigs: { codecResolution: number; frameRate: number }[] }
    ).videoConfigs[0]
    expect(cfg.codecResolution).toBe(2) // = vRes for the 1280×720 main tier
    expect(cfg.frameRate).toBe(2) // = vFps fallback (30)
  })
})
