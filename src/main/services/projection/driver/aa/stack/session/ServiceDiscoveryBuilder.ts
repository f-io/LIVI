import { DEBUG } from '@main/constants'
import {
  BT_PAIRING_METHOD,
  CH,
  DISPLAY_TYPE,
  MEDIA_CODEC,
  VIDEO_FPS,
  VIDEO_RESOLUTION
} from '../constants.js'
import type { ProtoTypes } from '../proto/index.js'
import type { SessionConfig, VideoCodec } from './Session.js'

function resolutionFromDimensions(w: number, h: number): number | null {
  if (w === 800 && h === 480) return VIDEO_RESOLUTION._800x480
  if (w === 1280 && h === 720) return VIDEO_RESOLUTION._1280x720
  if (w === 1920 && h === 1080) return VIDEO_RESOLUTION._1920x1080
  return null
}

export type ServiceDiscoveryResult = {
  buf: Buffer
  videoCodecByIndex: VideoCodec[]
  clusterCodecByIndex: VideoCodec[]
}

export function buildServiceDiscoveryResponse(
  cfg: SessionConfig,
  proto: ProtoTypes
): ServiceDiscoveryResult {
  const vW = cfg.videoWidth ?? 1280
  const vH = cfg.videoHeight ?? 720
  const dpi = cfg.videoDpi ?? 140
  const fps = cfg.videoFps ?? 30

  // VideoCodecResolutionType: 800x480=1, 1280x720=2, 1920x1080=3, 2560x1440=4, 3840x2160=5
  const vRes: number =
    vW >= 3840
      ? 5
      : vW >= 2560
        ? 4
        : vW >= 1920
          ? VIDEO_RESOLUTION._1920x1080
          : vW <= 800
            ? VIDEO_RESOLUTION._800x480
            : VIDEO_RESOLUTION._1280x720

  const vFps = fps === 60 ? VIDEO_FPS._60 : VIDEO_FPS._30

  let widthMargin = 0
  let heightMargin = 0
  if (cfg.displayWidth && cfg.displayHeight && vW > 0 && vH > 0) {
    const displayAR = cfg.displayWidth / cfg.displayHeight
    const tierAR = vW / vH
    if (displayAR > tierAR) {
      const contentH = Math.round(vW / displayAR) & ~1
      heightMargin = Math.max(0, vH - contentH)
    } else if (displayAR < tierAR) {
      const contentW = Math.round(vH * displayAR) & ~1
      widthMargin = Math.max(0, vW - contentW)
    }
  }

  const userTop = Math.max(0, cfg.mainSafeAreaTop ?? 0)
  const userBottom = Math.max(0, cfg.mainSafeAreaBottom ?? 0)
  const userLeft = Math.max(0, cfg.mainSafeAreaLeft ?? 0)
  const userRight = Math.max(0, cfg.mainSafeAreaRight ?? 0)
  const insetTop = Math.floor(heightMargin / 2) + userTop
  const insetBottom = heightMargin - Math.floor(heightMargin / 2) + userBottom
  const insetLeft = Math.floor(widthMargin / 2) + userLeft
  const insetRight = widthMargin - Math.floor(widthMargin / 2) + userRight

  // AudioStreamType: GUIDANCE=1, SYSTEM=2, MEDIA=3, TELEPHONY=4
  const AS_GUIDANCE = 1,
    AS_SYSTEM = 2,
    AS_MEDIA = 3,
    AS_TELEPHONY = 4

  const SENSOR = {
    LOCATION: 1,
    COMPASS: 2,
    SPEED: 3,
    RPM: 4,
    ODOMETER: 5,
    FUEL: 6,
    PARKING_BRAKE: 7,
    GEAR: 8,
    NIGHT_MODE: 10,
    ENV_DATA: 11,
    HVAC: 12,
    DRIVING_STATUS: 13,
    DOOR_DATA: 16,
    LIGHT_DATA: 17,
    TIRE_PRESSURE_DATA: 18,
    ACCELEROMETER: 19,
    GYROSCOPE: 20,
    GPS_SATELLITE: 21,
    VEHICLE_ENERGY_MODEL: 23,
    RAW_VEHICLE_ENERGY_MODEL: 25,
    RAW_EV_TRIP_SETTINGS: 26
  } as const

  const channels: object[] = []

  // ── Video (ch=3) ──
  const parE4 = cfg.pixelAspectRatioE4 ?? 10000
  const videoUiConfig = {
    margins: { top: insetTop, bottom: insetBottom, left: insetLeft, right: insetRight }
  }
  const baseVideoConfig = {
    codecResolution: vRes,
    frameRate: vFps,
    widthMargin,
    heightMargin,
    density: dpi,
    pixelAspectRatioE4: parE4,
    uiConfig: videoUiConfig
  }
  const videoConfigs: object[] = [{ ...baseVideoConfig, videoCodecType: MEDIA_CODEC.VIDEO_H264_BP }]
  const videoCodecByIndex: VideoCodec[] = ['h264']
  if (cfg.hevcSupported) {
    videoConfigs.push({ ...baseVideoConfig, videoCodecType: MEDIA_CODEC.VIDEO_H265 })
    videoCodecByIndex.push('h265')
  }
  if (DEBUG) {
    console.log(`[Session] advertising codecs: ${videoCodecByIndex.join(', ')}`)
  }
  channels.push({
    id: CH.VIDEO,
    mediaSinkService: {
      availableType: MEDIA_CODEC.VIDEO_H264_BP,
      availableWhileInCall: true,
      videoConfigs
    }
  })

  // ── Cluster Video (ch=19) — secondary display sink for Maps/Navi ──
  const clusterCodecByIndex: VideoCodec[] = []
  if (cfg.clusterEnabled) {
    const cW = cfg.clusterWidth ?? 0
    const cH = cfg.clusterHeight ?? 0
    const cTierW = cfg.clusterTierWidth ?? cW
    const cTierH = cfg.clusterTierHeight ?? cH
    const clusterRes = resolutionFromDimensions(cTierW, cTierH) ?? vRes
    const clusterFps =
      cfg.clusterFps === 60 ? VIDEO_FPS._60 : cfg.clusterFps === 30 ? VIDEO_FPS._30 : vFps
    const clusterDpi = cfg.clusterDpi ?? dpi

    let cWMargin = 0
    let cHMargin = 0
    if (cW > 0 && cH > 0 && cTierW > 0 && cTierH > 0) {
      const cAR = cW / cH
      const cTierAR = cTierW / cTierH
      if (cAR > cTierAR) {
        const contentH = Math.round(cTierW / cAR) & ~1
        cHMargin = Math.max(0, cTierH - contentH)
      } else if (cAR < cTierAR) {
        const contentW = Math.round(cTierH * cAR) & ~1
        cWMargin = Math.max(0, cTierW - contentW)
      }
    }

    const userClusterTop = Math.max(0, cfg.clusterSafeAreaTop ?? 0)
    const userClusterBottom = Math.max(0, cfg.clusterSafeAreaBottom ?? 0)
    const userClusterLeft = Math.max(0, cfg.clusterSafeAreaLeft ?? 0)
    const userClusterRight = Math.max(0, cfg.clusterSafeAreaRight ?? 0)
    const clusterMargins = {
      top: Math.floor(cHMargin / 2) + userClusterTop,
      bottom: cHMargin - Math.floor(cHMargin / 2) + userClusterBottom,
      left: Math.floor(cWMargin / 2) + userClusterLeft,
      right: cWMargin - Math.floor(cWMargin / 2) + userClusterRight
    }

    const clusterBase = {
      codecResolution: clusterRes,
      frameRate: clusterFps,
      widthMargin: cWMargin,
      heightMargin: cHMargin,
      density: clusterDpi,
      pixelAspectRatioE4: cfg.clusterPixelAspectRatioE4 ?? 10000,
      uiConfig: { margins: clusterMargins }
    }
    const clusterConfigs: object[] = [{ ...clusterBase, videoCodecType: MEDIA_CODEC.VIDEO_H264_BP }]
    clusterCodecByIndex.push('h264')
    if (cfg.hevcSupported) {
      clusterConfigs.push({ ...clusterBase, videoCodecType: MEDIA_CODEC.VIDEO_H265 })
      clusterCodecByIndex.push('h265')
    }
    if (cfg.vp9Supported) {
      clusterConfigs.push({ ...clusterBase, videoCodecType: MEDIA_CODEC.VIDEO_VP9 })
      clusterCodecByIndex.push('vp9')
    }
    if (cfg.av1Supported) {
      clusterConfigs.push({ ...clusterBase, videoCodecType: MEDIA_CODEC.VIDEO_AV1 })
      clusterCodecByIndex.push('av1')
    }
    channels.push({
      id: CH.CLUSTER_VIDEO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.VIDEO_H264_BP,
        availableWhileInCall: true,
        videoConfigs: clusterConfigs,
        displayType: DISPLAY_TYPE.CLUSTER,
        displayId: 1
      }
    })
    channels.push({
      id: CH.CLUSTER_INPUT,
      inputSourceService: { displayId: 1 }
    })
  }

  // ── Audio sinks + Microphone ──
  void AS_TELEPHONY

  if (!cfg.disableAudioOutput) {
    channels.push({
      id: CH.MEDIA_AUDIO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioType: AS_MEDIA,
        availableWhileInCall: true,
        audioConfigs: [{ samplingRate: 48000, numberOfBits: 16, numberOfChannels: 2 }]
      }
    })
    channels.push({
      id: CH.SPEECH_AUDIO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioType: AS_GUIDANCE,
        availableWhileInCall: true,
        audioConfigs: [{ samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 }]
      }
    })
  }

  channels.push({
    id: CH.SYSTEM_AUDIO,
    mediaSinkService: {
      availableType: MEDIA_CODEC.AUDIO_PCM,
      audioType: AS_SYSTEM,
      availableWhileInCall: true,
      audioConfigs: [{ samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 }]
    }
  })

  channels.push({
    id: CH.MIC_INPUT,
    mediaSourceService: {
      availableType: MEDIA_CODEC.AUDIO_PCM,
      audioConfig: { samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 },
      availableWhileInCall: true
    }
  })

  // ── Sensor Source (ch=1) ──
  const fuelTypes = cfg.fuelTypes && cfg.fuelTypes.length > 0 ? cfg.fuelTypes : [1]
  const evConnectorTypes = cfg.evConnectorTypes ?? []

  channels.push({
    id: CH.SENSOR,
    sensorSourceService: {
      sensors: [
        { sensorType: SENSOR.DRIVING_STATUS },
        { sensorType: SENSOR.LOCATION },
        { sensorType: SENSOR.NIGHT_MODE },
        { sensorType: SENSOR.SPEED },
        { sensorType: SENSOR.GEAR },
        { sensorType: SENSOR.PARKING_BRAKE },
        { sensorType: SENSOR.FUEL },
        { sensorType: SENSOR.ODOMETER },
        { sensorType: SENSOR.ENV_DATA },
        { sensorType: SENSOR.DOOR_DATA },
        { sensorType: SENSOR.LIGHT_DATA },
        { sensorType: SENSOR.TIRE_PRESSURE_DATA },
        { sensorType: SENSOR.HVAC },
        { sensorType: SENSOR.ACCELEROMETER },
        { sensorType: SENSOR.GYROSCOPE },
        { sensorType: SENSOR.COMPASS },
        { sensorType: SENSOR.GPS_SATELLITE },
        { sensorType: SENSOR.RPM },
        { sensorType: SENSOR.VEHICLE_ENERGY_MODEL },
        { sensorType: SENSOR.RAW_VEHICLE_ENERGY_MODEL },
        { sensorType: SENSOR.RAW_EV_TRIP_SETTINGS }
      ],
      // RAW_GPS_ONLY=256 | ACCEL=4 | GYRO=2 | COMPASS=8 | CAR_SPEED=64
      locationCharacterization: 256 | 4 | 2 | 8 | 64,
      supportedFuelTypes: fuelTypes,
      ...(evConnectorTypes.length > 0 ? { supportedEvConnectorTypes: evConnectorTypes } : {})
    }
  })

  // ── Input Source (ch=8) ──
  const touchW = Math.max(1, vW - insetLeft - insetRight)
  const touchH = Math.max(1, vH - insetTop - insetBottom)
  channels.push({
    id: CH.INPUT,
    inputSourceService: {
      keycodesSupported: [
        3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 21, 22, 23, 24, 25, 26, 66, 79, 82,
        84, 85, 86, 87, 88, 89, 90, 91, 111, 126, 127, 164, 219, 231, 260, 261, 262, 263, 65536
      ],
      touchscreen: [{ width: touchW, height: touchH }]
    }
  })

  // ── Bluetooth (ch=10) ──
  channels.push({
    id: CH.BLUETOOTH,
    bluetoothService: {
      carAddress: cfg.btMacAddress ?? '00:00:00:00:00:00',
      supportedPairingMethods: [BT_PAIRING_METHOD.PIN, BT_PAIRING_METHOD.NUMERIC_COMPARISON]
    }
  })

  // ── Navigation Status (ch=12) ──
  channels.push({
    id: CH.NAVIGATION,
    navigationStatusService: {
      minimumIntervalMs: 500,
      type: 1,
      imageOptions: { width: 256, height: 256, colourDepthBits: 32 }
    }
  })

  channels.push({ id: CH.MEDIA_INFO, mediaPlaybackService: {} })
  channels.push({ id: CH.PHONE_STATUS, phoneStatusService: {} })

  if (cfg.wifiBssid) {
    channels.push({
      id: CH.WIFI,
      wifiProjectionService: { carWifiBssid: cfg.wifiBssid }
    })
  }

  const sdrFields: Record<string, unknown> = {
    driverPosition: cfg.driverPosition ?? 0,
    displayName: cfg.huName ?? 'LIVI',
    probeForSupport: false,
    connectionConfiguration: {
      pingConfiguration: {
        timeoutMs: 5000,
        intervalMs: 1500,
        highLatencyThresholdMs: 500,
        trackedPingCount: 5
      }
    },
    headunitInfo: {
      make: 'LIVI',
      model: 'Universal',
      year: '2026',
      vehicleId: 'livi-001',
      headUnitMake: 'LIVI',
      headUnitModel: 'LIVI Head Unit',
      headUnitSoftwareBuild: '1',
      headUnitSoftwareVersion: '1.0'
    },
    make: 'LIVI',
    model: 'Universal',
    year: '2026',
    vehicleId: 'livi-001',
    headUnitMake: 'LIVI',
    headUnitModel: 'LIVI Head Unit',
    headUnitSoftwareBuild: '1',
    headUnitSoftwareVersion: '1.0',
    canPlayNativeMediaDuringVr: true,
    channels
  }

  const msg = proto.ServiceDiscoveryResponse.create(sdrFields)
  const buf = Buffer.from(proto.ServiceDiscoveryResponse.encode(msg).finish())

  if (DEBUG) {
    console.log(`[Session] SDR (aasdk aap_protobuf): ${channels.length} channels, ${buf.length}B`)
    console.log(
      `[Session] SDR hex: ${buf.subarray(0, 64).toString('hex')}${buf.length > 64 ? '...' : ''}`
    )
  }

  return { buf, videoCodecByIndex, clusterCodecByIndex }
}
