/**
 * CarlinKit dongle driver. The helper owns the USB pipe and the framing, streams
 * video and audio into the pipeline's feed and takes the microphone from its tap.
 * This side runs the protocol: open, box settings, work modes, pairing, and hands
 * the remaining messages to the app.
 */
import { DEBUG } from '@main/constants'
import { MicTap } from '@main/services/audio/micTap'
import { decryptVendorSessionText } from '@main/services/projection/driver/dongle/vendorSessionInfo'
import type { PendingStartupConnectTarget } from '@main/services/projection/services/types'
import {
  AudioData,
  BoxInfo,
  type BoxInfoSettings,
  DongleReady,
  DuckAudio,
  decodeTypeMap,
  Opened,
  PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo
} from '@projection/messages'
import {
  SendAutoConnectByBtAddress,
  type SendableMessage,
  SendBluetoothPairedList,
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone
} from '@projection/messages/sendable'
import type { Config } from '@shared/types'
import { InputCommand, MicType, PhoneWorkMode } from '@shared/types'
import type { CommandValue } from '@shared/types/ProjectionEnums'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import { isClusterDisplayed, matchFittingAAResolution } from '@shared/utils'
import EventEmitter from 'events'
import type { AaMediaSinkDeps } from '../aa/AaEventBridge'
import type { HelperSessionEvent, HelperSessionSource } from '../aa/AaManager'
import {
  type HelperSessionControl,
  HelperSessionLink
} from '../aa/stack/transport/HelperSessionLink'
import { DONGLE_MIC_TYPE } from './dongleConfig'
import { decodeMessage } from './protocol/decode.js'
import {
  encodeDongle,
  FileAddress,
  SendAndroidAutoDpi,
  SendBoolean,
  SendBoxSettings,
  SendFile,
  SendGnssData,
  SendIconConfig,
  SendNumber,
  SendOpen,
  SendSafeArea,
  SendString,
  SendViewArea
} from './protocol/sendables.js'
import type { MessageType } from './protocol/wire.js'

export const DONGLE_VENDOR_ID = 0x1314
const HELPER_RESUBSCRIBE_MS = 2000
/** With no phone connected this long after the open, the dongle is told to pair. */
const PAIR_AFTER_MS = 15000
const MIC_STOP_COMMANDS = new Set<number>([
  AudioCommand.AudioPhonecallStop,
  AudioCommand.AudioVoiceAssistantStop
])

export enum AndroidWorkMode {
  Off = 0,
  AndroidAuto = 1,
  CarLife = 2,
  AndroidMirror = 3,
  Search = 7
}

type UsbIds = { product?: number; version?: number; name?: string }

export type UsbDevice = {
  vendorId: number
  productId: number
  usbFwVersion: string
  deviceName: string
}

/** The device version the bus reports, as major.minor. */
function bcdVersion(v: number): string {
  return `${(v >> 8).toString(16)}.${(v & 0xff).toString(16).padStart(2, '0')}`
}

/** Tag of a host audio stream opened for one dongle format. */
function audioTag(decodeType: number, audioType: number): string {
  return `dongle:${decodeType}:${audioType}`
}

function parseAudioTag(tag: string | undefined): { decodeType: number; audioType: number } | null {
  const m = /^dongle:(\d+):(\d+)$/.exec(tag ?? '')
  return m ? { decodeType: Number(m[1]), audioType: Number(m[2]) } : null
}

export class DongleDriver extends EventEmitter {
  // Events: 'message', 'config-changed', 'failure', 'attached', 'detached',
  // 'phone-connected', 'phone-disconnected', 'dongle-info', 'targeted-connect-dispatched'
  private _helper: HelperSessionSource | null = null
  private _helperSub: { close: () => void } | null = null
  private _link: HelperSessionLink | null = null
  private _serial = ''
  private _productId: number | null = null
  private _usbVersion = ''
  private _deviceName = ''
  private _mediaSink: AaMediaSinkDeps | undefined
  private _offAudioOutput: (() => void) | null = null
  private _micPath = ''
  private _micTap: MicTap | null = null
  private _started = false
  private _videoSinkSent = false
  private _dongleFwVersion?: string
  private _boxInfo?: BoxInfoSettings
  private _lastDongleInfoEmitKey = ''
  private _cfg: Config | null = null
  private _postOpenConfigSent = false
  private _wifiConnectTimer: ReturnType<typeof setTimeout> | null = null
  private _pendingStartupConnectTarget: PendingStartupConnectTarget | null = null
  private _modeSwitchInFlight: Promise<void> = Promise.resolve()
  private _lastModeSwitchAt = 0

  // Runtime and initial mode
  private _androidWorkModeRuntime: AndroidWorkMode = AndroidWorkMode.AndroidAuto
  private _phoneWorkModeRuntime: PhoneWorkMode = PhoneWorkMode.CarPlay

  // centralised detection signals
  private _lastPluggedPhoneType: PhoneType | null = null
  private _linkUp = false
  private _frameTimer: ReturnType<typeof setInterval> | null = null
  private _pairTimer: ReturnType<typeof setTimeout> | null = null
  private _pendingModeHintFromBoxInfo: PhoneWorkMode | null = null
  private _duckNavActive = false
  private _duckVoiceActive = false

  // ── Sessions from the helper ───────────────────────────────────────────────

  attachHelper(helper: HelperSessionSource | undefined): void {
    if (this._helper || !helper) return
    this._helper = helper
    this._openHelperSub()
  }

  detachHelper(): void {
    this._helper = null
    const sub = this._helperSub
    this._helperSub = null
    try {
      sub?.close()
    } catch {
      /* already closed */
    }
    this._dropLink('helper gone')
  }

  private _openHelperSub(): void {
    const helper = this._helper
    if (!helper) return
    this._helperSub = helper.subscribe(
      (ev: HelperSessionEvent) => {
        if (ev.event !== 'dongle-session' || typeof ev.socket !== 'string') return
        const socket = ev.socket
        const serial = typeof ev.serial === 'string' ? ev.serial : ''
        HelperSessionLink.connect(socket, serial)
          .then((link) => {
            if (!this._helper) {
              link.destroy()
              return
            }
            this.attach(link, serial, { product: ev.product, version: ev.version, name: ev.name })
          })
          .catch((err: Error) => {
            console.warn(`[DongleDriver] helper session ${socket}: ${err.message}`)
          })
      },
      () => {
        this._helperSub = null
        if (this._helper) setTimeout(() => this._openHelperSub(), HELPER_RESUBSCRIBE_MS)
      }
    )
  }

  /** Takes over the session the helper opened for a dongle. */
  attach(link: HelperSessionLink, serial: string, ids?: UsbIds): void {
    if (this._link) this._dropLink('replaced by a new session')
    this._link = link
    this._serial = serial
    this._productId = typeof ids?.product === 'number' ? ids.product : null
    this._usbVersion = typeof ids?.version === 'number' ? bcdVersion(ids.version) : ''
    this._deviceName = typeof ids?.name === 'string' ? ids.name : ''
    link.on('message', (_ch: number, _flags: number, type: number, payload: Buffer) => {
      void this._onMessage(type, payload)
    })
    link.on('control', (c: HelperSessionControl) => this._onControl(c))
    link.on('error', (err: Error) => console.warn(`[DongleDriver] session error: ${err.message}`))
    link.on('close', () => {
      if (this._link === link) this._dropLink('session closed by the helper')
    })
    const sink = this._mediaSink
    if (sink) {
      this._offAudioOutput = sink.onAudioOutput((_audioType, streamId, tag) =>
        this._pushAudioSink(streamId, tag)
      )
      for (const o of sink.audioOutputs()) this._pushAudioSink(o.streamId, o.tag)
    }
    console.log(`[DongleDriver] dongle ${serial || '(no serial)'} attached`)
    this.emit('attached')
  }

  setMediaSink(sink: AaMediaSinkDeps): void {
    this._mediaSink = sink
  }

  get isUp(): boolean {
    return this._link !== null && !this._link.closed
  }

  get serial(): string {
    return this._serial
  }

  /** The dongle on the bus, while its session is up. */
  usbDevice(): UsbDevice | null {
    if (!this.isUp || this._productId == null) return null
    return {
      vendorId: DONGLE_VENDOR_ID,
      productId: this._productId,
      usbFwVersion: this._usbVersion,
      deviceName: this._deviceName
    }
  }

  /** Asks the helper to reset the dongle on the bus, the session ends with it. */
  resetDongle(): boolean {
    const link = this._link
    if (!link || link.closed) return false
    link.control({ type: 'reset' })
    return true
  }

  private _dropLink(reason: string): void {
    const link = this._link
    if (!link) return
    this._link = null
    console.log(`[DongleDriver] ${reason}`)
    link.destroy()
    this._offAudioOutput?.()
    this._offAudioOutput = null
    this._stopMic()
    this._clearTimers()
    const wasStarted = this._started
    this._started = false
    this._linkUp = false
    this._lastPluggedPhoneType = null
    this._pendingModeHintFromBoxInfo = null
    this._dongleFwVersion = undefined
    this._boxInfo = undefined
    this._lastDongleInfoEmitKey = ''
    this._postOpenConfigSent = false
    this._micPath = ''
    this._videoSinkSent = false
    if (wasStarted) this.emit('phone-disconnected')
    this.emit('detached')
  }

  // ── Logging ────────────────────────────────────────────────────────────────

  private logPhoneWorkModeChange(
    reason: string,
    from: PhoneWorkMode,
    to: PhoneWorkMode,
    extra?: string
  ) {
    console.log(
      `[DongleDriver] phone work mode change | reason=${reason} | from=${PhoneWorkMode[from]} | to=${PhoneWorkMode[to]}${extra ? ` | ${extra}` : ''}`
    )
  }

  private logAndroidWorkModeChange(
    reason: string,
    from: AndroidWorkMode,
    to: AndroidWorkMode,
    extra?: string
  ) {
    console.log(
      `[DongleDriver] android work mode change | reason=${reason} | from=${AndroidWorkMode[from]} | to=${AndroidWorkMode[to]}${extra ? ` | ${extra}` : ''}`
    )
  }

  // ── Work modes ─────────────────────────────────────────────────────────────

  private async applyAndroidWorkMode(next: AndroidWorkMode) {
    if (next === this._androidWorkModeRuntime) return
    this._androidWorkModeRuntime = next
    await this.send(new SendNumber(this._androidWorkModeRuntime, FileAddress.ANDROID_WORK_MODE))
    await this.send(new SendCommand('wifiEnable'))
    this.scheduleWifiConnect(150)
  }

  private resolveAndroidWorkModeOnPlugged(phoneType: PhoneType): AndroidWorkMode {
    if (phoneType === PhoneType.AndroidAuto) {
      return this._androidWorkModeRuntime === AndroidWorkMode.Off
        ? AndroidWorkMode.AndroidAuto
        : this._androidWorkModeRuntime
    }
    return this._androidWorkModeRuntime
  }

  private resolvePhoneWorkModeOnPlugged(phoneType: PhoneType): PhoneWorkMode {
    return phoneType === PhoneType.CarPlay ? PhoneWorkMode.CarPlay : PhoneWorkMode.Android
  }

  private async applyPhoneWorkMode(next: PhoneWorkMode) {
    const now = Date.now()
    if (next === this._phoneWorkModeRuntime) return
    if (now - this._lastModeSwitchAt < 800) return
    this._phoneWorkModeRuntime = next
    this._lastModeSwitchAt = now
    const cfg = this._cfg
    if (!cfg) return
    this._modeSwitchInFlight = this._modeSwitchInFlight.then(async () => {
      if (!this.isUp) return
      await this.send(new SendDisconnectPhone())
      await this.sleep(120)
      this._postOpenConfigSent = false
      await this.send(
        new SendOpen(
          { width: cfg.projectionWidth, height: cfg.projectionHeight, fps: cfg.projectionFps },
          this._phoneWorkModeRuntime
        )
      )
    })
    await this._modeSwitchInFlight
  }

  private scheduleWifiConnect(delayMs: number) {
    if (this._wifiConnectTimer) {
      clearTimeout(this._wifiConnectTimer)
      this._wifiConnectTimer = null
    }
    this._wifiConnectTimer = setTimeout(() => {
      void this.send(new SendCommand('wifiConnect'))
    }, delayMs)
  }

  public setPendingStartupConnectTarget(target: PendingStartupConnectTarget | null): void {
    if (!target) {
      this._pendingStartupConnectTarget = null
      return
    }
    const btMac = String(target.btMac ?? '').trim()
    if (!btMac) {
      this._pendingStartupConnectTarget = null
      return
    }
    this._pendingStartupConnectTarget = {
      btMac,
      phoneWorkMode: target.phoneWorkMode
    }
  }

  public clearPendingStartupConnectTarget(): void {
    this._pendingStartupConnectTarget = null
  }

  private sleep(ms: number) {
    return new Promise<void>((r) => setTimeout(r, ms))
  }

  private emitDongleInfoIfChanged() {
    const fw = this._dongleFwVersion
    const box = this._boxInfo
    let boxKey = ''
    if (box != null) {
      try {
        boxKey = JSON.stringify(box)
      } catch {
        boxKey = String(box)
      }
    }
    const key = `${fw ?? ''}||${boxKey}`
    if (key === this._lastDongleInfoEmitKey) return
    this._lastDongleInfoEmitKey = key
    this.emit('dongle-info', { dongleFwVersion: fw, boxInfo: box })
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  /** Opens the dongle with the projection geometry, once per session. */
  start = async (
    cfg: Config,
    pendingTarget?: PendingStartupConnectTarget | null
  ): Promise<void> => {
    if (!this.isUp || this._started) return
    this._started = true
    this._cfg = cfg
    if (pendingTarget) this.setPendingStartupConnectTarget(pendingTarget)
    else this.clearPendingStartupConnectTarget()
    this._phoneWorkModeRuntime =
      cfg.lastPhoneWorkMode === PhoneWorkMode.Android
        ? PhoneWorkMode.Android
        : PhoneWorkMode.CarPlay
    this._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    this._postOpenConfigSent = false
    this._videoSinkSent = false
    // The dongle only ever produces H.264.
    this.emit('video-codec', 'h264')
    this.emit('cluster-video-codec', 'h264')
    await this.send(
      new SendOpen(
        { width: cfg.projectionWidth, height: cfg.projectionHeight, fps: cfg.projectionFps },
        this._phoneWorkModeRuntime
      )
    )
    await this.sleep(120)
    this._clearPairTimer()
    this._pairTimer = setTimeout(() => {
      void this.send(new SendCommand('wifiPair'))
    }, PAIR_AFTER_MS)
  }

  close = async (): Promise<void> => {
    this._dropLink('closed')
  }

  send = async (msg: SendableMessage): Promise<boolean> => {
    const link = this._link
    if (!link || link.closed) return false
    try {
      const { type, payload } = encodeDongle(msg)
      link.send(0, 0, type, payload)
      return true
    } catch (err) {
      console.error('[DongleDriver] Send error', msg?.constructor?.name, err)
      return false
    }
  }

  public sendBluetoothPairedList = async (listText: string): Promise<boolean> => {
    return this.send(new SendBluetoothPairedList(listText))
  }

  public sendGnssData = async (nmeaText: string): Promise<boolean> => {
    return this.send(new SendGnssData(nmeaText))
  }

  uploadHostIcons(icon120: Buffer, icon180: Buffer, icon256: Buffer): void {
    void this.send(new SendFile(icon120, FileAddress.ICON_120))
    void this.send(new SendFile(icon180, FileAddress.ICON_180))
    void this.send(new SendFile(icon256, FileAddress.ICON_256))
  }

  requestClusterFocus(): void {
    void this.send(new SendCommand('requestClusterStreamFocus'))
  }

  handleInput = (command: InputCommand): void => {
    const map: Partial<Record<InputCommand, CommandValue>> = {
      [InputCommand.Play]: 'play',
      [InputCommand.Pause]: 'pause',
      [InputCommand.PlayPause]: 'playPause',
      [InputCommand.Next]: 'next',
      [InputCommand.Previous]: 'prev'
    }
    const value = map[command]
    if (!value) {
      if (DEBUG) console.log(`[DongleDriver] handleInput: no dongle mapping for ${command}`)
      return
    }
    void this.send(new SendCommand(value))
  }

  requestKeyframe(): void {
    void this.send(new SendCommand('frame'))
  }

  setStreamVolume(audioType: number, level: number, rampMs: number): void {
    this._mediaSink?.setHostVolume(audioType, level, rampMs)
  }

  disconnectPhone = async (): Promise<boolean> => {
    let ok = false
    try {
      ok = (await this.send(new SendDisconnectPhone())) || ok
    } catch (e) {
      console.warn('[DongleDriver] SendDisconnectPhone failed', e)
    }
    try {
      ok = (await this.send(new SendCloseDongle())) || ok
    } catch (e) {
      console.warn('[DongleDriver] SendCloseDongle failed', e)
    }
    if (ok) await this.sleep(150)
    return ok
  }

  // ── Media sinks ────────────────────────────────────────────────────────────

  /** Tells the helper which planes take the video. */
  private _pushVideoSink(): void {
    const sink = this._mediaSink
    const cfg = this._cfg
    if (!sink || !cfg) return
    sink.primeVideo(false)
    const video = [{ cluster: false, id: sink.videoPlaneId(false), codec: 'h264' }]
    if (isClusterDisplayed(cfg)) {
      sink.primeVideo(true)
      video.push({ cluster: true, id: sink.videoPlaneId(true), codec: 'h264' })
    }
    void sink.feedPath().then((feed) => {
      if (!this.isUp) return
      if (!feed) console.warn('[DongleDriver] host has no media feed, video will not show')
      this._link?.control({ type: 'sink', feed, video })
    })
  }

  /** Routes a host stream to the dongle format it was opened for. */
  private _pushAudioSink(streamId: number, tag: string | undefined): void {
    const sink = this._mediaSink
    const format = parseAudioTag(tag)
    if (!sink || !format) return
    void sink.feedPath().then((feed) => {
      if (!this.isUp) return
      this._link?.control({ type: 'sink', feed, audio: [{ ...format, id: streamId }] })
    })
  }

  // ── Microphone ─────────────────────────────────────────────────────────────

  private _startMic(decodeType: number): void {
    if (this._micTap) return
    const fmt = decodeTypeMap[decodeType]
    if (!fmt || !this._micPath) {
      console.warn(`[DongleDriver] no microphone for decode type ${decodeType}`)
      return
    }
    this._micTap = MicTap.open(this._micPath, {
      sampleRate: fmt.frequency,
      channels: fmt.channel,
      device: this._cfg?.audioInputDevice || undefined
    })
    if (!this._micTap) {
      console.warn('[DongleDriver] microphone tap could not open')
      return
    }
    this._link?.control({ type: 'mic', decodeType })
    console.log(`[DongleDriver] microphone on, ${fmt.frequency} Hz ${fmt.channel} ch`)
  }

  private _stopMic(): void {
    const tap = this._micTap
    if (!tap) return
    this._micTap = null
    try {
      tap.close()
    } catch (err) {
      console.warn(`[DongleDriver] microphone tap close failed: ${(err as Error).message}`)
    }
    if (this.isUp) this._link?.control({ type: 'mic' })
    console.log('[DongleDriver] microphone off')
  }

  // ── Messages ───────────────────────────────────────────────────────────────

  private async _onMessage(type: number, payload: Buffer): Promise<void> {
    let msg: unknown
    try {
      msg = decodeMessage(type as MessageType, payload.length ? payload : undefined)
    } catch (err) {
      console.warn(
        `[DongleDriver] message 0x${type.toString(16)} not decodable: ${(err as Error).message}`
      )
      return
    }
    if (msg) await this.handleMessage(msg)
  }

  private _onControl(c: HelperSessionControl): void {
    switch (c.type) {
      case 'ready':
        this._micPath = typeof c.mic === 'string' ? c.mic : ''
        break
      case 'video': {
        const cluster = c.cluster === true
        const width = typeof c.width === 'number' ? c.width : 0
        const height = typeof c.height === 'number' ? c.height : 0
        if (!this._videoSinkSent) {
          this._videoSinkSent = true
          this._pushVideoSink()
        }
        if (!cluster && !this._linkUp) {
          this._linkUp = true
          console.log('[DongleDriver] first frame, link up')
          this.emit('phone-connected')
        }
        this._mediaSink?.noteVideoStarted(cluster, width, height)
        break
      }
      case 'audio-setup': {
        const decodeType = typeof c.decodeType === 'number' ? c.decodeType : 0
        const audioType = typeof c.audioType === 'number' ? c.audioType : 0
        const fmt = decodeTypeMap[decodeType]
        if (!fmt) {
          console.warn(
            `[DongleDriver] audio decode type ${decodeType} unknown, stream stays silent`
          )
          break
        }
        this._mediaSink?.primeAudio(
          audioType,
          fmt.frequency,
          fmt.channel,
          audioTag(decodeType, audioType)
        )
        break
      }
      case 'closed':
        this._dropLink(`session ended: ${typeof c.reason === 'string' ? c.reason : 'unknown'}`)
        break
    }
  }

  private translateDuck(cmd: number): void {
    switch (cmd) {
      case AudioCommand.AudioNaviStart:
      case AudioCommand.AudioTurnByTurnStart:
        this._duckNavActive = true
        break
      case AudioCommand.AudioNaviStop:
      case AudioCommand.AudioTurnByTurnStop:
        this._duckNavActive = false
        break
      case AudioCommand.AudioVoiceAssistantStart:
      case AudioCommand.AudioPhonecallStart:
        this._duckVoiceActive = true
        break
      case AudioCommand.AudioVoiceAssistantStop:
      case AudioCommand.AudioPhonecallStop:
        this._duckVoiceActive = false
        break
      default:
        return
    }
    const level = this._duckVoiceActive ? 0 : this._duckNavActive ? 0.2 : 1
    this.emit('message', new DuckAudio(level, level < 1 ? 500 : 1500))
  }

  private async handleMessage(msg: unknown) {
    if (msg instanceof VendorSessionInfo) {
      try {
        const decrypted = await decryptVendorSessionText(msg.raw)
        if (DEBUG) {
          console.log(`[DongleDriver] VendorSessionInfo ${decrypted}`)
        }
      } catch (e) {
        console.warn('[DongleDriver] VendorSessionInfo decrypt failed', e)
      }
      this.emit('message', msg)
      return
    }
    if (msg instanceof DongleReady) {
      console.log('[DongleDriver] Dongle ready')
      this.emit('message', msg)
      return
    }
    if (msg instanceof SoftwareVersion) {
      this._dongleFwVersion = msg.version
      this.emitDongleInfoIfChanged()
    }
    if (msg instanceof BoxInfo) {
      await this.onBoxInfo(msg)
      this.emit('message', msg)
      return
    }
    this.emit('message', msg)
    if (msg instanceof AudioData && msg.command != null) {
      this.translateDuck(msg.command)
      if (msg.command === AudioCommand.AudioInputConfig) this._startMic(msg.decodeType)
      else if (MIC_STOP_COMMANDS.has(msg.command)) this._stopMic()
    }
    if (msg instanceof Opened) this.onOpened()
    if (msg instanceof Unplugged) this.onUnplugged()
    if (msg instanceof Plugged) await this.onPlugged(msg)
  }

  private onOpened() {
    void this.sendPostOpenConfig()
  }

  private async sendPostOpenConfig() {
    if (this._postOpenConfigSent) return
    const cfg = this._cfg
    if (!cfg) return
    if (!this.isUp) return

    const ui = (cfg.oemName ?? '').trim()
    const label = ui.length > 0 ? ui : cfg.carName

    const initMicRouteCommand: CommandValue =
      DONGLE_MIC_TYPE === MicType.DongleMic
        ? 'boxMici2s'
        : DONGLE_MIC_TYPE === MicType.PhoneMic
          ? 'phoneMic'
          : 'mic'

    const aaResolution = matchFittingAAResolution({
      width: cfg.projectionWidth,
      height: cfg.projectionHeight
    })

    const projectionAreaMessages: SendableMessage[] = [
      new SendViewArea(cfg.projectionWidth, cfg.projectionHeight, {
        insets: {
          top: cfg.projectionViewAreaTop,
          bottom: cfg.projectionViewAreaBottom,
          left: cfg.projectionViewAreaLeft,
          right: cfg.projectionViewAreaRight
        }
      }),
      // Safe area is additive to the view area, the dongle requires safe area to
      // stay inside the view area (Safe Area is a subset of View Area).
      new SendSafeArea(cfg.projectionWidth, cfg.projectionHeight, {
        insets: {
          top: cfg.projectionViewAreaTop + cfg.projectionSafeAreaTop,
          bottom: cfg.projectionViewAreaBottom + cfg.projectionSafeAreaBottom,
          left: cfg.projectionViewAreaLeft + cfg.projectionSafeAreaLeft,
          right: cfg.projectionViewAreaRight + cfg.projectionSafeAreaRight
        },
        drawOutside: cfg.projectionSafeAreaDrawOutside
      })
    ]

    // Cluster view/safe area files must follow SendBoxSettings, which makes the
    // dongle rewrite the HU_NAVISCREEN_*_INFO files at full size first.
    const clusterAreaMessages: SendableMessage[] = isClusterDisplayed(cfg)
      ? [
          new SendViewArea(cfg.clusterWidth, cfg.clusterHeight, {
            address: FileAddress.HU_NAVISCREEN_VIEWAREA_INFO,
            insets: {
              top: cfg.clusterViewAreaTop,
              bottom: cfg.clusterViewAreaBottom,
              left: cfg.clusterViewAreaLeft,
              right: cfg.clusterViewAreaRight
            }
          }),
          new SendSafeArea(cfg.clusterWidth, cfg.clusterHeight, {
            address: FileAddress.HU_NAVISCREEN_SAFEAREA_INFO,
            insets: {
              top: cfg.clusterViewAreaTop + cfg.clusterSafeAreaTop,
              bottom: cfg.clusterViewAreaBottom + cfg.clusterSafeAreaBottom,
              left: cfg.clusterViewAreaLeft + cfg.clusterSafeAreaLeft,
              right: cfg.clusterViewAreaRight + cfg.clusterSafeAreaRight
            },
            drawOutside: false
          })
        ]
      : []

    const messages: SendableMessage[] = [
      ...projectionAreaMessages,
      new SendBoxSettings(cfg),
      ...clusterAreaMessages,
      new SendString(label, FileAddress.BOX_NAME),
      new SendBoolean(cfg.nightMode, FileAddress.NIGHT_MODE),
      new SendAndroidAutoDpi(aaResolution.width, aaResolution.height),
      new SendNumber(this._androidWorkModeRuntime, FileAddress.ANDROID_WORK_MODE),
      new SendBoolean(true, FileAddress.CHARGE_MODE),
      new SendIconConfig({ oemName: cfg.oemName }),
      new SendNumber(cfg.hand, FileAddress.HAND_DRIVE_MODE),
      new SendCommand(initMicRouteCommand),
      new SendCommand(cfg.wifiType === '5ghz' ? 'wifi5g' : 'wifi24g'),
      new SendCommand(cfg.disableAudioOutput ? 'audioTransferOn' : 'audioTransferOff')
    ]

    for (const m of messages) {
      await this.send(m)
      await this.sleep(120)
    }

    const pendingTarget = this._pendingStartupConnectTarget
    if (pendingTarget) {
      if (this._wifiConnectTimer) {
        clearTimeout(this._wifiConnectTimer)
        this._wifiConnectTimer = null
      }
      if (DEBUG) {
        console.debug('[DongleDriver] sendPostOpenConfig uses targeted auto-connect', {
          btMac: pendingTarget.btMac,
          phoneWorkMode: pendingTarget.phoneWorkMode
        })
      }
      await this.send(new SendAutoConnectByBtAddress(pendingTarget.btMac))
      this.emit('targeted-connect-dispatched', pendingTarget)
      this._pendingStartupConnectTarget = null
    } else {
      this.scheduleWifiConnect(150)
    }
    this._postOpenConfigSent = true
  }

  private onUnplugged() {
    this._lastPluggedPhoneType = null
    this._linkUp = false
    this._pendingModeHintFromBoxInfo = null
    this._clearFrameTimer()
    this._stopMic()
    this.emit('phone-disconnected')
  }

  private async onPlugged(msg: Plugged) {
    this._clearPairTimer()
    this._lastPluggedPhoneType = msg.phoneType

    const frameInterval = this._cfg?.phoneConfig?.[msg.phoneType]?.frameInterval
    if (frameInterval && frameInterval > 0 && !this._frameTimer) {
      this._frameTimer = setInterval(() => {
        if (this._started) void this.send(new SendCommand('frame'))
      }, frameInterval)
    }

    await this.reconcileModes('plugged')

    const cfg = this._cfg
    if (cfg) {
      const connectedMode = this.resolvePhoneWorkModeOnPlugged(msg.phoneType)
      if (cfg.lastPhoneWorkMode !== connectedMode) {
        cfg.lastPhoneWorkMode = connectedMode
        this.emit('config-changed', { lastPhoneWorkMode: connectedMode })
      }
    }
    console.log('[DongleDriver] dongle plugged (awaiting first frame)')
  }

  private async onBoxInfo(msg: BoxInfo) {
    this._boxInfo = msg.settings
    this.emitDongleInfoIfChanged()

    // IMPORTANT: Do not react to empty MDLinkType (dongle still initializing)
    const md = String(msg.settings.MDLinkType ?? '')

    // Flip ONLY on the explicit mismatch signal !! WARNING: Chinese TYPO!
    if (md === 'RiddleLinktype_UNKNOWN?' || md === 'RiddleLinktype_UNKOWN?') {
      const current = this._phoneWorkModeRuntime
      const next = current === PhoneWorkMode.Android ? PhoneWorkMode.CarPlay : PhoneWorkMode.Android

      // Only flip if cfg exists
      if (this._cfg) {
        this.logPhoneWorkModeChange(md, current, next)
        await this.applyPhoneWorkMode(next)
      }
    }
    this.emitDongleInfoIfChanged()
  }

  private async reconcileModes(reason: 'plugged' | 'boxinfo') {
    // Decide desired modes from signals
    let desiredPhone: PhoneWorkMode | null = null
    let desiredAndroid: AndroidWorkMode | null = null

    if (this._lastPluggedPhoneType != null) {
      desiredPhone = this.resolvePhoneWorkModeOnPlugged(this._lastPluggedPhoneType)
      desiredAndroid = this.resolveAndroidWorkModeOnPlugged(this._lastPluggedPhoneType)
    } else if (this._pendingModeHintFromBoxInfo != null) {
      desiredPhone = this._pendingModeHintFromBoxInfo
      desiredAndroid = null // don't touch android work mode
    }

    // Apply phone mode ONLY via applyPhoneWorkMode (single authority)
    if (desiredPhone != null && desiredPhone !== this._phoneWorkModeRuntime) {
      this.logPhoneWorkModeChange(reason, this._phoneWorkModeRuntime, desiredPhone)
      await this.applyPhoneWorkMode(desiredPhone)
    }

    // Apply android work mode ONLY via applyAndroidWorkMode
    if (desiredAndroid != null && desiredAndroid !== this._androidWorkModeRuntime) {
      this.logAndroidWorkModeChange(reason, this._androidWorkModeRuntime, desiredAndroid)
      await this.applyAndroidWorkMode(desiredAndroid)
    }
  }

  // ── Timers ─────────────────────────────────────────────────────────────────

  private _clearFrameTimer(): void {
    if (this._frameTimer) {
      clearInterval(this._frameTimer)
      this._frameTimer = null
    }
  }

  private _clearPairTimer(): void {
    if (this._pairTimer) {
      clearTimeout(this._pairTimer)
      this._pairTimer = null
    }
  }

  private _clearTimers(): void {
    this._clearFrameTimer()
    this._clearPairTimer()
    if (this._wifiConnectTimer) {
      clearTimeout(this._wifiConnectTimer)
      this._wifiConnectTimer = null
    }
  }
}
