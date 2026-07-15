/**
 * CpSession — IPhoneDriver for ONE Apple CarPlay connection.
 *
 * Wraps a single CpStack that adopts an already-accepted :7000 control socket via
 * attachSocket (no listener of its own). CpManager owns the shared infra (the
 * :7000 listener, the MFi signer, the helper event feed) and constructs one
 * CpSession per accepted connection. The session bridges its stack's stream
 * events into the driver contract (video/audio/message/device-presence) and
 * ingests the per-phone iAP2 metadata the manager routes to it.
 */

import { EventEmitter } from 'node:events'
import type * as net from 'node:net'
import { Microphone } from '@main/services/audio'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { Config } from '@shared/types'
import type { InputCommand } from '@shared/types/InputCommand'
import {
  AudioCommand,
  CommandMapping,
  MultiTouchAction,
  TouchAction
} from '@shared/types/ProjectionEnums'
import { isClusterDisplayed } from '@shared/utils'
import { MessageHeader, MessageType } from '../../messages/common'
import {
  buildAlbumArtMessage,
  buildMediaJsonMessage,
  buildNaviJsonMessage
} from '../../messages/metaBuilders'
import { AudioData, Command } from '../../messages/readable'
import {
  type SendableMessage,
  SendCommand,
  SendMultiTouch,
  SendTouch
} from '../../messages/sendable'
import { buildVideoDataMessage } from '../aa/AaEventBridge'
import { detectBtMac } from '../aa/stack/system/hwaddr'
import type { IPhoneDriver } from '../IPhoneDriver'
import type { CpHelperSock } from './CpHelperSock'
import { CpStack } from './stack/cpStack'
import { MediaButton, TelephonyButton } from './stack/hid'
import type { CpAudioProfile, CpIcon, CpStackConfig, CpStreamProfile } from './stack/types'

/** Full knob-axis deflection: a d-pad direction maps to X/Y at the extreme (±127). */
const KNOB_DEFLECT = 127

/** Build a LIVI AudioData (PCM) message, matching the AA/dongle wire format. */
function buildCpAudioData(
  pcm: Buffer,
  sampleRate: number,
  channels: number,
  audioType: number
): AudioData {
  const HEADER = 12
  const sampleBytes = pcm.length - (pcm.length % 2)
  const data = Buffer.allocUnsafeSlow(HEADER + sampleBytes)
  data.writeUInt32LE(0, 0)
  data.writeFloatLE(0, 4)
  data.writeUInt32LE(audioType, 8)
  pcm.copy(data, HEADER, 0, sampleBytes)
  const msg = new AudioData(new MessageHeader(data.length, MessageType.AudioData), data)
  msg.sampleRate = sampleRate
  msg.channels = channels
  return msg
}

/** Build a LIVI AudioData carrying a 1-byte AudioCommand (stream start/stop). */
function buildCpAudioCommand(prof: CpAudioProfile, active: boolean): AudioData {
  const HEADER = 12
  const data = Buffer.allocUnsafeSlow(HEADER + 1)
  data.writeUInt32LE(prof.decodeType, 0)
  data.writeFloatLE(0, 4)
  data.writeUInt32LE(prof.audioType, 8)
  data.writeUInt8(active ? prof.startCmd : prof.stopCmd, HEADER)
  return new AudioData(new MessageHeader(data.length, MessageType.AudioData), data)
}

function buildCpCallCommand(command: AudioCommand): AudioData {
  const HEADER = 12
  const data = Buffer.allocUnsafeSlow(HEADER + 1)
  data.writeUInt32LE(5, 0)
  data.writeFloatLE(0, 4)
  data.writeUInt32LE(2, 8)
  data.writeUInt8(command, HEADER)
  return new AudioData(new MessageHeader(data.length, MessageType.AudioData), data)
}

export interface CpSessionSeed {
  hevcSupported: boolean
  initialNightMode: boolean | undefined
  clusterStreamActive: boolean
}

export interface CpSessionOptions {
  socket?: net.Socket
  getConfig: () => Config
  /** Shared MFi coprocessor + BlueZ control socket, owned by CpManager. */
  helper: CpHelperSock
  seed: CpSessionSeed
}

export class CpSession extends EventEmitter implements IPhoneDriver {
  private _stack: CpStack | null = null
  private _closed = false
  private _connected = false
  private _downEmitted = false
  private _hevc: boolean
  private _initialNightMode: boolean | undefined
  private _mic: Microphone | null = null
  private _micActive = false
  private readonly _getConfig: () => Config
  private readonly _helper: CpHelperSock
  /** Phone identity, learned from the stack's session-level SETUP + the socket peer. */
  private _btMac = ''
  private _wifiMac = ''
  private _peerIp = ''
  private _usbUdid = ''

  constructor(opts: CpSessionOptions) {
    super()
    this._getConfig = opts.getConfig
    this._helper = opts.helper
    this._hevc = opts.seed.hevcSupported
    this._initialNightMode = opts.seed.initialNightMode

    const stack = new CpStack(this._buildStackConfig(this._getConfig()))
    this._stack = stack
    stack.setConfigRefresh(() =>
      stack.applyDisplayConfig(this._buildStackConfig(this._getConfig()))
    )
    if (this._initialNightMode !== undefined) stack.setNightMode(this._initialNightMode)
    this._wireStack(stack)
    stack.setClusterStreamActive(opts.seed.clusterStreamActive)

    this.on('disconnected', () => {
      setImmediate(() => {
        void this.close()
      })
    })

    if (opts.socket) this.attachSocket(opts.socket)
  }

  /** Adopt the accepted :7000 control socket. Split from construction so a CpSession can be
   *  born at iAP2 identification and gain its AirPlay transport when the phone connects. */
  attachSocket(socket: net.Socket): void {
    this._peerIp = normHost(socket.remoteAddress ?? '')
    this._stack?.attachSocket(socket)
  }

  async start(_cfg: Config): Promise<boolean> {
    return true
  }

  /** CarPlay is wireless-only here; the wired-carkit label is tracked by ProjectionService. */
  isWiredMode(): boolean {
    return false
  }

  setHevcSupported(supported: boolean): void {
    this._hevc = supported
  }
  setVp9Supported(_supported: boolean): void {}
  setAv1Supported(_supported: boolean): void {}

  setInitialNightMode(value: boolean | undefined): void {
    this._initialNightMode = value
    if (value !== undefined) this._stack?.setNightMode(value)
  }

  sendNightMode(night: boolean): void {
    this._stack?.setNightMode(night)
  }

  setClusterStreamActive(active: boolean): void {
    this._stack?.setClusterStreamActive(active)
  }

  requestKeyframe(): void {
    this._stack?.forceMainKeyframe()
    this._stack?.forceClusterKeyframe()
  }

  /** The live CarPlay session's stable pair-verify controller id, the CP device identity. */
  getControllerId(): string | null {
    return this._stack?.activeControllerId ?? null
  }

  getBtMac(): string {
    return this._btMac
  }

  get peerIp(): string {
    return this._peerIp
  }

  /** True when a helper metadata/presence event's identity belongs to this session. */
  matchesIdentity(ids: {
    btMac?: string
    wifiMac?: string
    ip?: string
    usbUdid?: string
    controllerId?: string
  }): boolean {
    const cid = this.getControllerId()
    return Boolean(
      (ids.btMac && this._btMac && ids.btMac.toLowerCase() === this._btMac.toLowerCase()) ||
        (ids.wifiMac &&
          this._wifiMac &&
          ids.wifiMac.toLowerCase() === this._wifiMac.toLowerCase()) ||
        (ids.ip && this._peerIp && normHost(ids.ip) === this._peerIp) ||
        (ids.usbUdid && this._usbUdid && ids.usbUdid === this._usbUdid) ||
        (ids.controllerId && cid && ids.controllerId === cid)
    )
  }

  /** Adopt a registry-level identity (Bonjour/carkit) matched to this phone by MAC,
   *  so the session gains the phone's Wi-Fi IP for byDevice correlation. */
  adoptHelperDevice(ids: { btMac?: string; ip?: string; usbUdid?: string; name?: string }): void {
    if (ids.btMac) this._btMac = ids.btMac
    if (ids.usbUdid) this._usbUdid = ids.usbUdid
    this.emit('device-presence', {
      kind: 'device',
      btMac: ids.btMac || this._btMac,
      wifiMac: this._wifiMac,
      ip: ids.ip ?? '',
      usbUdid: ids.usbUdid ?? '',
      name: ids.name ?? ''
    })
  }

  private _emitDisconnected(): void {
    if (this._downEmitted) return
    this._downEmitted = true
    this._connected = false
    this.emit('disconnected')
  }

  private _wireStack(stack: CpStack): void {
    stack.on('error', (err: Error) => console.warn(`[CpSession] stack error: ${err.message}`))
    stack.on('session-active', (ip: string) => {
      this.emit('device-presence', { kind: 'active', ip })
    })
    stack.on(
      'device-info',
      (d: { name?: string; deviceId?: string; wifiMac?: string; model?: string }) => {
        if (!this._btMac && d.deviceId) this._btMac = d.deviceId
        this._wifiMac = d.wifiMac ?? ''
        this.emit('identity', { btMac: this._btMac, wifiMac: this._wifiMac })
        this.emit('device-presence', {
          kind: 'device',
          btMac: this._btMac,
          wifiMac: this._wifiMac,
          name: d.name ?? '',
          model: d.model ?? ''
        })
      }
    )
    stack.on('session-ended', () => {
      // The phone's control connection dropped. Tear the session down; onCpDisconnected
      // closes it and releases the video plane. Fires 'disconnected' exactly once.
      this._emitDisconnected()
    })
    stack.on('video-codec', (codec: string) => this.emit('video-codec', codec))
    stack.on('audio-frame', (pcm: Buffer, prof: CpStreamProfile) => {
      this.emit('message', buildCpAudioData(pcm, prof.sampleRate, prof.channels, prof.audioType))
    })
    stack.on('audio-active', (prof: CpAudioProfile, active: boolean) => {
      this.emit('message', buildCpAudioCommand(prof, active))
    })
    stack.on('mic-active', (active: boolean, sampleRate: number, channels: number) => {
      if (active) this._startMicCapture(sampleRate, channels)
      else this._stopMicCapture()
    })
    stack.on('host-ui-requested', () => {
      const buf = Buffer.allocUnsafe(4)
      buf.writeUInt32LE(CommandMapping.requestHostUI, 0)
      this.emit('message', new Command(new MessageHeader(buf.length, MessageType.Command), buf))
    })
    stack.on('speech-active', (active: boolean) => {
      const buf = Buffer.allocUnsafe(4)
      buf.writeUInt32LE(
        active ? CommandMapping.voiceAssistantUiActive : CommandMapping.voiceAssistantUiIdle,
        0
      )
      this.emit('message', new Command(new MessageHeader(buf.length, MessageType.Command), buf))
    })
    stack.on('disable-bluetooth', (deviceID: string) => {
      const mac = deviceID.trim()
      if (!mac) return
      this._helper
        .disconnectBt(mac)
        .then(() => console.log(`[CpSession] BT disconnected for ${mac} — iAP2 moves to tunnel`))
        .catch((e: Error) => console.warn(`[CpSession] disconnectBt failed: ${e.message}`))
    })
    stack.on('video-frame', (nal: Buffer) => {
      const cfg = this._getConfig()
      const w = cfg.projectionWidth || 1920
      const h = cfg.projectionHeight || 1080
      if (!this._connected) {
        this._connected = true
        this.emit('connected')
      }
      this.emit('message', buildVideoDataMessage(nal, w, h))
    })
    stack.on('cluster-video-codec', (codec: string) => this.emit('cluster-video-codec', codec))
    stack.on('cluster-video-frame', (nal: Buffer) => {
      const cfg = this._getConfig()
      const w = cfg.clusterWidth || 1280
      const h = cfg.clusterHeight || 720
      this.emit('message', buildVideoDataMessage(nal, w, h, MessageType.ClusterVideoData))
    })
  }

  /**
   * Ingest one per-phone iAP2 metadata event routed here by CpManager (nowplaying,
   * navigation, call-state, battery, cellular, album art). Presence identity
   * (wifi/device) is handled by the manager, never here.
   */
  ingestHelperEvent(ev: Record<string, unknown>): void {
    if (ev.type === 'albumart') {
      if (typeof ev.dataB64 === 'string') {
        const buf = Buffer.from(ev.dataB64, 'base64')
        if (buf.length > 0) this.emit('message', buildAlbumArtMessage(buf))
      }
      return
    }
    if (ev.type === 'navigation') {
      this._onNavigationEvent(ev)
      return
    }
    if (ev.type === 'power') {
      this.emit('device-presence', {
        kind: 'status',
        batteryLevel: typeof ev.level === 'number' ? ev.level : undefined,
        batteryCharging: typeof ev.charging === 'boolean' ? ev.charging : undefined
      })
      return
    }
    if (ev.type === 'cellular') {
      this.emit('device-presence', {
        kind: 'status',
        signalStrength: typeof ev.signal === 'number' ? ev.signal : undefined,
        carrierName: typeof ev.carrier === 'string' ? ev.carrier : undefined
      })
      return
    }
    if (ev.type === 'call') {
      if (ev.phase === 'ringing') {
        this.emit('message', buildCpCallCommand(AudioCommand.AudioAttentionRinging))
      } else if (ev.phase === 'active') {
        this.emit('message', buildCpCallCommand(AudioCommand.AudioPhonecallStart))
      } else if (ev.phase === 'ended') {
        this.emit('message', buildCpCallCommand(AudioCommand.AudioPhonecallStop))
      }
      return
    }
    if (ev.type !== 'nowplaying') return
    const media: Record<string, unknown> = {}
    if (typeof ev.title === 'string') media.MediaSongName = ev.title
    if (typeof ev.artist === 'string') media.MediaArtistName = ev.artist
    if (typeof ev.album === 'string') media.MediaAlbumName = ev.album
    if (typeof ev.appName === 'string') media.MediaAPPName = ev.appName
    if (typeof ev.durationMs === 'number') media.MediaSongDuration = ev.durationMs
    if (typeof ev.elapsedMs === 'number') media.MediaSongPlayTime = ev.elapsedMs
    if (typeof ev.playing === 'number') media.MediaPlayStatus = ev.playing
    if (Object.keys(media).length > 0) {
      this.emit('message', buildMediaJsonMessage(media))
    }
  }

  private _onNavigationEvent(ev: Record<string, unknown>): void {
    const navi: Record<string, unknown> = {}
    if (typeof ev.status === 'number') navi.NaviStatus = ev.status === 0 ? 0 : 1
    if (typeof ev.orderType === 'number') navi.NaviOrderType = ev.orderType
    if (typeof ev.roadName === 'string') navi.NaviRoadName = ev.roadName
    if (typeof ev.destinationName === 'string') navi.NaviDestinationName = ev.destinationName
    if (typeof ev.timeToDestination === 'number') navi.NaviTimeToDestination = ev.timeToDestination
    if (typeof ev.distanceToDestination === 'number')
      navi.NaviDistanceToDestination = ev.distanceToDestination
    if (typeof ev.remainDistance === 'number') navi.NaviRemainDistance = ev.remainDistance
    if (typeof ev.maneuverType === 'number') navi.NaviManeuverType = ev.maneuverType
    if (typeof ev.turnSide === 'number') navi.NaviTurnSide = ev.turnSide
    if (typeof ev.junctionType === 'number') navi.NaviJunctionType = ev.junctionType
    if (typeof ev.turnAngle === 'number') navi.NaviTurnAngle = ev.turnAngle
    if (typeof ev.etaEpoch === 'number' && ev.etaEpoch > 0) {
      const d = new Date(ev.etaEpoch * 1000)
      navi.NaviETA = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
    }
    if (Object.keys(navi).length > 0) {
      this.emit('message', buildNaviJsonMessage(navi))
    }
  }

  private _startMicCapture(sampleRate: number, channels: number): void {
    if (this._micActive) return
    this._micActive = true
    if (!this._mic) {
      this._mic = new Microphone()
      this._mic.on('data', (chunk: Buffer) => {
        if (this._micActive) this._stack?.writeMic(chunk)
      })
    }
    console.log(`[CpSession] mic uplink → starting capture (${sampleRate}Hz ${channels}ch)`)
    this._mic.start(5, { frequency: sampleRate, channels })
  }

  private _stopMicCapture(): void {
    if (!this._micActive) return
    this._micActive = false
    console.log('[CpSession] mic uplink → stopping capture')
    this._mic?.stop()
  }

  async close(): Promise<void> {
    if (this._closed) return
    this._closed = true
    this._emitDisconnected()
    this._micActive = false
    this._mic?.stop()
    this._mic = null
    try {
      this._stack?.stop()
    } catch (e) {
      console.warn(`[CpSession] stack stop threw: ${(e as Error).message}`)
    }
    this._stack = null
  }

  async send(msg: SendableMessage): Promise<boolean> {
    if (!this._stack) return false
    const clamp = (v: number): number => Math.min(1, Math.max(0, v))
    if (msg instanceof SendTouch) {
      this._stack.sendTouches([
        { id: 0, x: clamp(msg.x), y: clamp(msg.y), down: msg.action !== TouchAction.Up }
      ])
      return true
    }
    if (msg instanceof SendMultiTouch) {
      // Map each pointer to a HID contact (index as the contact id).
      this._stack.sendTouches(
        msg.touches.map((t, i) => ({
          id: i,
          x: clamp(t.x),
          y: clamp(t.y),
          down: t.action !== MultiTouchAction.Up
        }))
      )
      return true
    }
    if (msg instanceof SendCommand) {
      return this._sendCommand(msg.value)
    }
    return false
  }

  private _sendCommand(cmd: CommandMapping): boolean {
    const stack = this._stack
    if (!stack) return false
    switch (cmd) {
      case CommandMapping.play:
        stack.sendMedia(MediaButton.play)
        return true
      case CommandMapping.pause:
        stack.sendMedia(MediaButton.pause)
        return true
      case CommandMapping.playPause:
        stack.sendMedia(MediaButton.playPause)
        return true
      case CommandMapping.next:
        stack.sendMedia(MediaButton.next)
        return true
      case CommandMapping.prev:
        stack.sendMedia(MediaButton.prev)
        return true
      case CommandMapping.left:
        stack.sendKnob({ x: -KNOB_DEFLECT })
        return true
      case CommandMapping.right:
        stack.sendKnob({ x: KNOB_DEFLECT })
        return true
      case CommandMapping.up:
        stack.sendKnob({ y: -KNOB_DEFLECT })
        return true
      case CommandMapping.down:
        stack.sendKnob({ y: KNOB_DEFLECT })
        return true
      case CommandMapping.selectDown:
      case CommandMapping.knobDown:
        stack.sendKnobSelect(true)
        return true
      case CommandMapping.selectUp:
      case CommandMapping.knobUp:
        stack.sendKnobSelect(false)
        return true
      case CommandMapping.back:
        stack.sendKnob({ back: true })
        return true
      case CommandMapping.home:
        stack.sendKnob({ home: true })
        return true
      case CommandMapping.knobLeft:
        stack.sendKnob({ wheel: -1 })
        return true
      case CommandMapping.knobRight:
        stack.sendKnob({ wheel: 1 })
        return true
      case CommandMapping.acceptPhone:
        stack.sendTelephony(TelephonyButton.hookSwitch)
        return true
      case CommandMapping.rejectPhone:
        stack.sendTelephony(TelephonyButton.drop)
        return true
      case CommandMapping.phoneKey0:
        stack.sendTelephony(TelephonyButton.key0)
        return true
      case CommandMapping.phoneKey1:
        stack.sendTelephony(TelephonyButton.key1)
        return true
      case CommandMapping.phoneKey2:
        stack.sendTelephony(TelephonyButton.key2)
        return true
      case CommandMapping.phoneKey3:
        stack.sendTelephony(TelephonyButton.key3)
        return true
      case CommandMapping.phoneKey4:
        stack.sendTelephony(TelephonyButton.key4)
        return true
      case CommandMapping.phoneKey5:
        stack.sendTelephony(TelephonyButton.key5)
        return true
      case CommandMapping.phoneKey6:
        stack.sendTelephony(TelephonyButton.key6)
        return true
      case CommandMapping.phoneKey7:
        stack.sendTelephony(TelephonyButton.key7)
        return true
      case CommandMapping.phoneKey8:
        stack.sendTelephony(TelephonyButton.key8)
        return true
      case CommandMapping.phoneKey9:
        stack.sendTelephony(TelephonyButton.key9)
        return true
      case CommandMapping.phoneKeyStar:
        stack.sendTelephony(TelephonyButton.star)
        return true
      case CommandMapping.phoneKeyHash:
        stack.sendTelephony(TelephonyButton.pound)
        return true
      case CommandMapping.phoneKeyHookSwitch:
        stack.sendTelephony(TelephonyButton.hookSwitch)
        return true
      case CommandMapping.voiceAssistant:
        stack.invokeSiri()
        return true
      case CommandMapping.voiceAssistantRelease:
        // Voice activation: the release is a no-op (this is not push-to-talk).
        return true
      default:
        return false
    }
  }

  handleInput(_command: InputCommand): void {}

  /** OEM icons for the CarPlay homescreen: user upload (config) or the LIVI default. */
  private _buildIcons(cfg: Config): CpIcon[] {
    const sizes: { size: number; b64: string }[] = [
      { size: 120, b64: cfg.dongleIcon120?.trim() || ICON_120_B64 },
      { size: 180, b64: cfg.dongleIcon180?.trim() || ICON_180_B64 },
      { size: 256, b64: cfg.dongleIcon256?.trim() || ICON_256_B64 }
    ]
    return sizes
      .map(({ size, b64 }) => ({
        widthPixels: size,
        heightPixels: size,
        data: Buffer.from(b64.trim(), 'base64')
      }))
      .filter((icon) => icon.data.length > 0)
  }

  private _buildStackConfig(cfg: Config): CpStackConfig {
    const name = cfg.carName?.trim() ? cfg.carName : 'LIVI'
    return {
      deviceName: name,
      oemLabel: cfg.oemName?.trim() ? cfg.oemName : name,
      icons: this._buildIcons(cfg),
      // deviceID must be the accessory's real BT adapter MAC: CarPlay uses it to
      // correlate the Bluetooth bond with the CarPlay session, so the phone recognises
      // the car and auto-reconnects (pair-verify) instead of re-pairing every time.
      deviceId: detectBtMac(cfg.btAdapter || undefined) ?? 'AA:BB:CC:DD:EE:FF',
      // AirPlay protocol version. 320.17 (AirLib) made the phone stop POSTing
      // /feedback entirely (it switched to a newer timing mechanism we don't handle),
      // which broke the media clock. 280.33.8 keeps the phone polling /feedback.
      sourceVersion: '280.33.8',
      hevc: this._hevc,
      h264: !this._hevc,
      main: {
        widthPixels: cfg.projectionWidth || 1920,
        heightPixels: cfg.projectionHeight || 1080,
        fps: cfg.projectionFps || 60,
        primaryInputDevice: 1,
        viewArea: {
          top: cfg.projectionViewAreaTop,
          bottom: cfg.projectionViewAreaBottom,
          left: cfg.projectionViewAreaLeft,
          right: cfg.projectionViewAreaRight
        },
        safeArea: {
          top: cfg.projectionSafeAreaTop,
          bottom: cfg.projectionSafeAreaBottom,
          left: cfg.projectionSafeAreaLeft,
          right: cfg.projectionSafeAreaRight
        },
        safeAreaDrawOutside: cfg.projectionSafeAreaDrawOutside
      },
      cluster: isClusterDisplayed(cfg)
        ? {
            widthPixels: cfg.clusterWidth,
            heightPixels: cfg.clusterHeight,
            fps: cfg.projectionFps || 60,
            viewArea: {
              top: cfg.clusterViewAreaTop,
              bottom: cfg.clusterViewAreaBottom,
              left: cfg.clusterViewAreaLeft,
              right: cfg.clusterViewAreaRight
            },
            safeArea: {
              top: cfg.clusterSafeAreaTop,
              bottom: cfg.clusterSafeAreaBottom,
              left: cfg.clusterSafeAreaLeft,
              right: cfg.clusterSafeAreaRight
            }
          }
        : undefined,
      port: 7000,
      // samplingFrequency: 1 = 48 kHz, 0 = 44.1 kHz — drives the advertised
      // entertainment (type 102) AAC-LC rate so the phone streams at the user's choice.
      entertainmentSampleRate: cfg.samplingFrequency === 1 ? 48000 : 44100,
      disableAudioOutput: Boolean(cfg.disableAudioOutput),
      mfi: this._helper
    }
  }
}

/** Strip an IPv6 zone id and the ::ffff: v4-mapped prefix so peers compare equal. */
function normHost(h: string): string {
  return h ? h.replace(/%.*$/, '').replace(/^::ffff:/i, '') : ''
}

export default CpSession
