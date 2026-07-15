import { DEBUG } from '@main/constants'
import { decryptVendorSessionText } from '@main/helpers/vendorSessionInfo'
import type { PendingStartupConnectTarget } from '@main/services/projection/services/types'
import { CARLINKIT_PIDS, CARLINKIT_VID, isCarlinkitDongle } from '@main/services/usb/constants'
import { HeaderBuildError, MessageHeader } from '@projection/messages/common'
import {
  BluetoothPeerConnected,
  BoxInfo,
  type BoxInfoSettings,
  DongleReady,
  Opened,
  PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo,
  VideoData
} from '@projection/messages/readable'
import {
  FileAddress,
  HeartBeat,
  SendAndroidAutoDpi,
  SendAudio,
  SendAutoConnectByBtAddress,
  SendableMessage,
  SendBluetoothPairedList,
  SendBoolean,
  SendBoxSettings,
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendFile,
  SendGnssData,
  SendIconConfig,
  SendNumber,
  SendOpen,
  SendSafeArea,
  SendString,
  SendViewArea
} from '@projection/messages/sendable'
import { asDomUSBDevice } from '@projection/services/utils/asDomUSBDevice'
import type { Config } from '@shared/types'
import { InputCommand, MicType, PhoneWorkMode } from '@shared/types'
import type { CommandValue } from '@shared/types/ProjectionEnums'
import { isClusterDisplayed, matchFittingAAResolution } from '@shared/utils'
import EventEmitter from 'events'
import { usb } from 'usb'

const CONFIG_NUMBER = 1
const MAX_ERROR_COUNT = 5
const READ_TIMEOUT_MS = 1_000

type UnknownRecord = Record<string, unknown>

function isRecord(v: unknown): v is UnknownRecord {
  return typeof v === 'object' && v !== null
}

function readProp<T = unknown>(obj: unknown, key: string): T | undefined {
  if (!isRecord(obj)) return undefined
  return obj[key] as T
}

export enum AndroidWorkMode {
  Off = 0,
  AndroidAuto = 1,
  CarLife = 2,
  AndroidMirror = 3,
  Search = 7
}

export class DriverStateError extends Error {}

export class DongleDriver extends EventEmitter {
  private _heartbeatInterval: ReturnType<typeof setInterval> | null = null
  private _device: USBDevice | null = null
  private _inEP: USBEndpoint | null = null
  private _outEP: USBEndpoint | null = null
  private _ifaceNumber: number | null = null

  private errorCount = 0
  private _closing = false
  private _started = false
  private _readerActive = false
  private _closePromise: Promise<void> | null = null

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
  private _videoFlowing = false
  private _frameTimer: ReturnType<typeof setInterval> | null = null
  private _pairTimer: ReturnType<typeof setTimeout> | null = null
  private _pendingModeHintFromBoxInfo: PhoneWorkMode | null = null

  // Logging PhoneWorkMode
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

  // Logging AndroidWorkMode
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
      if (this._closing || !this._device?.opened) return

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

  static knownDevices = CARLINKIT_PIDS.map((productId) => ({
    vendorId: CARLINKIT_VID,
    productId
  }))

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

  private async waitForReaderStop(timeoutMs = 1500) {
    const t0 = Date.now()
    while (this._readerActive && Date.now() - t0 < timeoutMs) {
      await this.sleep(10)
    }
  }

  // The device follows the WebUSB-shaped API (usb@3 / nusb).
  private isBenignUsbShutdownError(err: unknown): boolean {
    const msg = err instanceof Error ? err.message : String(err)

    // Typical macOS/libusb shutdown / unplug / reset fallout.
    return (
      msg.includes('LIBUSB_ERROR_NO_DEVICE') ||
      msg.includes('LIBUSB_ERROR_NOT_FOUND') ||
      msg.includes('LIBUSB_TRANSFER_NO_DEVICE') ||
      msg.includes('LIBUSB_TRANSFER_ERROR') ||
      msg.includes('transferIn error') ||
      msg.includes('device has been disconnected') ||
      msg.includes('No such device')
    )
  }

  private async tryResetUnderlyingUsbDevice(dev: USBDevice): Promise<boolean> {
    const candidates: unknown[] = [
      readProp(dev, 'device'),
      readProp(dev, '_device'),
      readProp(dev, 'usbDevice'),
      readProp(dev, 'rawDevice')
    ]

    const raw = candidates.find(isRecord)
    if (!raw) return false

    const resetFn = readProp(raw, 'reset')
    if (typeof resetFn !== 'function') return false

    try {
      await new Promise<void>((resolve, reject) => {
        ;(resetFn as (cb: (err: unknown) => void) => void).call(raw, (err: unknown) =>
          err ? reject(err) : resolve()
        )
      })
      return true
    } catch (e) {
      console.warn('[DongleDriver] underlying usb reset() failed', e)
      return false
    }
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

  get isUp(): boolean {
    return this._device !== null
  }

  bringUp = async (
    cfg: Config,
    pendingTarget?: PendingStartupConnectTarget | null
  ): Promise<void> => {
    if (this._device) return
    const device = (await usb.getDevices()).find((d) => isCarlinkitDongle(d.vendorId, d.productId))
    if (!device) return
    try {
      await device.open()
      await this.initialise(asDomUSBDevice(device))
      if (pendingTarget) this.setPendingStartupConnectTarget(pendingTarget)
      else this.clearPendingStartupConnectTarget()
      await this.start(cfg)
      this._pairTimer = setTimeout(() => {
        void this.send(new SendCommand('wifiPair'))
      }, 15000)
    } catch (e) {
      console.warn('[DongleDriver] bring-up failed', e)
      await this.close()
    }
  }

  initialise = async (device: USBDevice) => {
    if (this._device) return

    try {
      this._device = device
      if (!device.opened) throw new DriverStateError('Device not opened')

      await device.selectConfiguration(CONFIG_NUMBER)
      const cfg = device.configuration
      if (!cfg) throw new DriverStateError('Device has no configuration')

      const intf = cfg.interfaces[0]
      if (!intf) throw new DriverStateError('No interface 0')

      this._ifaceNumber = intf.interfaceNumber
      await device.claimInterface(this._ifaceNumber)

      const alt = intf.alternate
      if (!alt) throw new DriverStateError('No active alternate on interface')

      this._inEP = alt.endpoints.find((e) => e.direction === 'in') || null
      this._outEP = alt.endpoints.find((e) => e.direction === 'out') || null
      if (!this._inEP || !this._outEP) throw new DriverStateError('Endpoints missing')
      if (!this._readerActive) void this.readLoop()
    } catch (err) {
      await this.close()
      throw err
    }
  }

  send = async (msg: SendableMessage): Promise<boolean> => {
    const dev = this._device
    if (!dev || !dev.opened || this._closing) return false
    if (!this._outEP) return false

    try {
      const buf = msg.serialise()
      const view = new Uint8Array(buf.buffer as ArrayBuffer, buf.byteOffset, buf.byteLength)
      const res = await dev.transferOut(this._outEP.endpointNumber, view)
      return res.status === 'ok'
    } catch (err) {
      console.error('[DongleDriver] Send error', msg?.constructor?.name, err)
      const detail = err instanceof Error ? err.message : String(err)
      if (/disconnect/i.test(detail)) void this.close()
      return false
    }
  }

  public sendBluetoothPairedList = async (listText: string): Promise<boolean> => {
    return this.send(new SendBluetoothPairedList(listText))
  }

  public sendGnssData = async (nmeaText: string): Promise<boolean> => {
    return this.send(new SendGnssData(nmeaText))
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

  private _frameWatchdog(): void {
    if (this._frameTimer) return
    this._frameTimer = setInterval(() => {
      if (!this._started) return
      if (this._videoFlowing) {
        this._videoFlowing = false
        return
      }
      void this.send(new SendCommand('frame'))
    }, 700)
  }

  private _clearFramePoke(): void {
    if (this._frameTimer) {
      clearInterval(this._frameTimer)
      this._frameTimer = null
    }
    this._videoFlowing = false
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

  sendPhoneAudio(pcm: Int16Array, decodeType: number): void {
    void this.send(new SendAudio(pcm, decodeType))
  }

  uploadHostIcons(icon120: Buffer, icon180: Buffer, icon256: Buffer): void {
    void this.send(new SendFile(icon120, FileAddress.ICON_120))
    void this.send(new SendFile(icon180, FileAddress.ICON_180))
    void this.send(new SendFile(icon256, FileAddress.ICON_256))
  }

  requestClusterFocus(): void {
    void this.send(new SendCommand('requestClusterStreamFocus'))
  }

  // isolate framing/decoding
  private async readOneMessage() {
    const dev = this._device
    const inEp = this._inEP
    if (!dev || !inEp) return null

    const transferIn = dev.transferIn.bind(dev) as (
      ep: number,
      len: number,
      timeoutMs?: number
    ) => Promise<USBInTransferResult>

    const headerRes = await transferIn(
      inEp.endpointNumber,
      MessageHeader.dataLength,
      READ_TIMEOUT_MS
    )
    if (this._closing) return null

    const headerData = headerRes?.data
    if (!headerData) throw new HeaderBuildError('Empty header')

    const headerBuffer = Buffer.from(
      headerData.buffer,
      headerData.byteOffset,
      headerData.byteLength
    )
    const header = MessageHeader.fromBuffer(headerBuffer)

    let extra: Buffer | undefined
    if (header.length) {
      const extraRes = await transferIn(inEp.endpointNumber, header.length, READ_TIMEOUT_MS)
      if (this._closing) return null
      const extraData = extraRes?.data
      if (!extraData) throw new Error('Failed to read extra data')
      extra = Buffer.from(extraData.buffer, extraData.byteOffset, extraData.byteLength)
    }

    return header.toMessage(extra)
  }

  // entral message dispatch
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

    // Track info
    if (msg instanceof SoftwareVersion) {
      this._dongleFwVersion = msg.version
      this.emitDongleInfoIfChanged()
    }

    // BoxInfo: store + signal extraction + reconcile
    if (msg instanceof BoxInfo) {
      await this.onBoxInfo(msg)
      this.emit('message', msg)
      return
    }

    if (msg instanceof VideoData) {
      this._videoFlowing = true
      this._frameWatchdog()
      if (!this._linkUp) {
        this._linkUp = true
        console.log('[DongleDriver] first frame — link up')
        this.emit('phone-connected')
      }
    }

    // Everything else: emit raw first
    this.emit('message', msg)

    if (msg instanceof BluetoothPeerConnected) {
      // intentionally no-op
    }

    if (msg instanceof Opened) this.onOpened()
    if (msg instanceof Unplugged) this.onUnplugged()
    if (msg instanceof Plugged) await this.onPlugged(msg)
  }

  private onOpened() {
    if (!this._heartbeatInterval) {
      this._heartbeatInterval = setInterval(() => void this.send(new HeartBeat()), 2000)
    }
    void this.sendPostOpenConfig()
  }

  private async sendPostOpenConfig() {
    if (this._postOpenConfigSent) return

    const cfg = this._cfg
    if (!cfg) return
    if (this._closing || !this._device?.opened) return

    const ui = (cfg.oemName ?? '').trim()
    const label = ui.length > 0 ? ui : cfg.carName
    const initMicRouteCommand: CommandValue =
      cfg.micType === MicType.DongleMic
        ? 'boxMici2s'
        : cfg.micType === MicType.PhoneMic
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
    this._clearFramePoke()

    if (this._heartbeatInterval) {
      clearInterval(this._heartbeatInterval)
      this._heartbeatInterval = null
    }
    this.emit('phone-disconnected')
  }

  private async onPlugged(msg: Plugged) {
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

  // --- readLoop rewritten to be simple ---
  private async readLoop() {
    if (this._readerActive) return
    this._readerActive = true

    try {
      while (this._device?.opened && !this._closing) {
        if (this.errorCount >= MAX_ERROR_COUNT) {
          await this.close()
          this.emit('failure')
          return
        }

        try {
          const msg = await this.readOneMessage()
          if (!msg) continue

          await this.handleMessage(msg)

          if (this.errorCount !== 0) this.errorCount = 0
        } catch (err) {
          if (this._closing || !this._device?.opened) break

          const msg = err instanceof Error ? err.message : String(err)
          // Idle read timeout / cancel by the timeout — re-issue (lossless for bulk).
          if (/cancel|timed?\s*out|timeout/i.test(msg)) continue
          // Device really went away.
          if (this.isBenignUsbShutdownError(err)) break

          if (err instanceof HeaderBuildError) {
            console.warn('[DongleDriver] HeaderBuildError', err.message)
          } else {
            console.error('[DongleDriver] readLoop error', err)
          }

          this.errorCount++
        }
      }
    } finally {
      this._readerActive = false
    }
  }

  start = async (cfg: Config) => {
    if (!this._device) throw new DriverStateError('initialise() first')
    if (!this._device.opened) return
    if (this._started) return

    this.errorCount = 0
    this._started = true
    this._cfg = cfg

    this._phoneWorkModeRuntime =
      cfg.lastPhoneWorkMode === PhoneWorkMode.Android
        ? PhoneWorkMode.Android
        : PhoneWorkMode.CarPlay
    this._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto

    this._postOpenConfigSent = false

    const messages: SendableMessage[] = [
      new SendOpen(
        { width: cfg.projectionWidth, height: cfg.projectionHeight, fps: cfg.projectionFps },
        this._phoneWorkModeRuntime
      )
    ]

    for (const m of messages) {
      await this.send(m)
      await this.sleep(120)
    }
  }

  close = async (): Promise<void> => {
    // Serialize close() calls
    if (this._closePromise) return this._closePromise

    this._closePromise = (async () => {
      // Nothing to do?
      if (!this._device && !this._readerActive && !this._started) return

      const wasPresent = this._device !== null
      this._closing = true
      this._clearFramePoke()

      if (this._pairTimer) {
        clearTimeout(this._pairTimer)
        this._pairTimer = null
      }

      if (this._wifiConnectTimer) {
        clearTimeout(this._wifiConnectTimer)
        this._wifiConnectTimer = null
      }

      if (this._heartbeatInterval) {
        clearInterval(this._heartbeatInterval)
        this._heartbeatInterval = null
      }

      const dev = this._device
      const iface = this._ifaceNumber

      // If we end up in the "pending request" situation, we may intentionally keep the device ref.
      let keepDeviceRefToAvoidGcFinalizerCrash = false

      try {
        if (dev && dev.opened) {
          // _closing is set; the read loop drops its in-flight read on the next timeout and exits.
          // Wait for it so the interface is free (a pending transfer blocks releaseInterface/close).
          await this.waitForReaderStop(READ_TIMEOUT_MS + 500)

          if (iface != null) {
            try {
              await dev.releaseInterface(iface)
            } catch (e) {
              console.warn('[DongleDriver] releaseInterface() failed (ignored)', e)
            }
          }

          try {
            await dev.close()
          } catch (e: unknown) {
            const msg = e instanceof Error ? e.message : String(e)

            if (/pending request/i.test(msg)) {
              console.warn(
                '[DongleDriver] device.close(): pending request -> trying underlying usb reset()'
              )

              // Try to cancel libusb I/O at the raw level
              const resetOk = await this.tryResetUnderlyingUsbDevice(dev)
              if (resetOk) {
                await this.sleep(50)
                await this.waitForReaderStop(1500)
              }

              // Try close once more (best-effort)
              try {
                await dev.close()
              } catch (e2: unknown) {
                const msg2 = e2 instanceof Error ? e2.message : String(e2)
                if (/pending request/i.test(msg2)) {
                  console.warn(
                    '[DongleDriver] device.close(): pending request did not resolve before deadline'
                  )
                  // Intentionally keep reference: avoids GC finalizer calling libusb_close later
                  keepDeviceRefToAvoidGcFinalizerCrash = true
                } else {
                  console.warn('[DongleDriver] device.close() failed', e2)
                }
              }
            } else {
              console.warn('[DongleDriver] device.close() failed', e)
            }
          }
        }
      } catch (err) {
        console.warn('[DongleDriver] close() outer error', err)
      } finally {
        // Always reset logical state
        this._heartbeatInterval = null
        this._inEP = null
        this._outEP = null
        this._ifaceNumber = null
        this._started = false
        this._readerActive = false
        this.errorCount = 0

        this._dongleFwVersion = undefined
        this._boxInfo = undefined
        this._lastDongleInfoEmitKey = ''
        this._postOpenConfigSent = false

        // Only clear the device ref if we successfully closed OR we are sure it won't crash later.
        if (!keepDeviceRefToAvoidGcFinalizerCrash) {
          this._device = null
        }

        this._closing = false
      }

      if (wasPresent) this.emit('phone-disconnected')
    })().finally(() => {
      this._closePromise = null
    })

    return this._closePromise
  }
}
