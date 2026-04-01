import {
  AndroidWorkMode,
  DongleDriver,
  DriverStateError
} from '@main/services/projection/driver/DongleDriver'
import {
  SendAutoConnectByBtAddress,
  SendBluetoothPairedList,
  SendCommand,
  SendDisconnectPhone,
  SendGnssData,
  SendOpen,
  SendString
} from '@main/services/projection/messages/sendable'
import { MicType, PhoneWorkMode, CommandMapping } from '@shared/types'
import {
  BluetoothPeerConnected,
  BoxInfo,
  DongleReady,
  Opened,
  PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo
} from '@main/services/projection/messages/readable'
import {
  HeaderBuildError,
  MessageHeader,
  MessageType
} from '@main/services/projection/messages/common'

jest.mock('@main/helpers/vendorSessionInfo', () => ({
  decryptVendorSessionText: jest.fn(async () => 'decrypted-session')
}))

describe('DongleDriver core behavior', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  test('emitDongleInfoIfChanged emits only when payload key changes', () => {
    const d = new DongleDriver() as any
    const onInfo = jest.fn()
    d.on('dongle-info', onInfo)

    d._dongleFwVersion = '1.0.0'
    d._boxInfo = { productType: 'A15W' }

    d.emitDongleInfoIfChanged()
    d.emitDongleInfoIfChanged()

    expect(onInfo).toHaveBeenCalledTimes(1)
    expect(onInfo).toHaveBeenCalledWith({
      dongleFwVersion: '1.0.0',
      boxInfo: { productType: 'A15W' }
    })
  })

  test('scheduleWifiConnect debounces timers and sends wifiConnect command once', async () => {
    const d = new DongleDriver() as any
    d.send = jest.fn(async () => true)

    d.scheduleWifiConnect(100)
    d.scheduleWifiConnect(200)

    jest.advanceTimersByTime(200)
    await Promise.resolve()

    expect(d.send).toHaveBeenCalledTimes(1)
    expect(d.send.mock.calls[0][0]).toBeInstanceOf(SendCommand)
  })

  test('applyAndroidWorkMode no-ops when mode unchanged', async () => {
    const d = new DongleDriver() as any
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)

    await d.applyAndroidWorkMode(AndroidWorkMode.AndroidAuto)

    expect(d.send).not.toHaveBeenCalled()
  })

  test('applyAndroidWorkMode updates mode and sends config + wifi enable', async () => {
    const d = new DongleDriver() as any
    d._androidWorkModeRuntime = AndroidWorkMode.Off
    d.send = jest.fn(async () => true)

    await d.applyAndroidWorkMode(AndroidWorkMode.AndroidAuto)

    expect(d._androidWorkModeRuntime).toBe(AndroidWorkMode.AndroidAuto)
    expect(d.send).toHaveBeenCalledTimes(2)
  })

  test('resolveAndroidWorkModeOnPlugged keeps runtime mode for AndroidAuto unless runtime is Off', () => {
    const d = new DongleDriver() as any

    d._androidWorkModeRuntime = AndroidWorkMode.Search
    expect(d.resolveAndroidWorkModeOnPlugged(PhoneType.AndroidAuto)).toBe(AndroidWorkMode.Search)

    d._androidWorkModeRuntime = AndroidWorkMode.Off
    expect(d.resolveAndroidWorkModeOnPlugged(PhoneType.AndroidAuto)).toBe(
      AndroidWorkMode.AndroidAuto
    )
  })

  test('resolveAndroidWorkModeOnPlugged leaves mode unchanged for non-AndroidAuto phones', () => {
    const d = new DongleDriver() as any
    d._androidWorkModeRuntime = AndroidWorkMode.CarLife

    expect(d.resolveAndroidWorkModeOnPlugged(PhoneType.CarPlay)).toBe(AndroidWorkMode.CarLife)
  })

  test('resolvePhoneWorkModeOnPlugged maps CarPlay and Android correctly', () => {
    const d = new DongleDriver() as any

    expect(d.resolvePhoneWorkModeOnPlugged(PhoneType.CarPlay)).toBe(PhoneWorkMode.CarPlay)
    expect(d.resolvePhoneWorkModeOnPlugged(PhoneType.AndroidAuto)).toBe(PhoneWorkMode.Android)
  })

  test('send returns false when no device exists', async () => {
    const d = new DongleDriver()

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(false)
  })

  test('send returns false when device is closed', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: false }
    d._outEP = { endpointNumber: 1 }

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(false)
  })

  test('send returns false when closing or missing out endpoint', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = true
    d._outEP = { endpointNumber: 1 }

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(false)

    d._closing = false
    d._outEP = null

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(false)
  })

  test('send transfers serialized message and returns true on ok status', async () => {
    const d = new DongleDriver() as any
    const transferOut = jest.fn(async () => ({ status: 'ok' }))

    d._device = { opened: true, transferOut }
    d._outEP = { endpointNumber: 7 }
    d._closing = false

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(true)
    expect(transferOut).toHaveBeenCalledWith(7, expect.any(Uint8Array))
  })

  test('send returns false on transfer error', async () => {
    const d = new DongleDriver() as any
    const transferOut = jest.fn(async () => {
      throw new Error('boom')
    })

    d._device = { opened: true, transferOut }
    d._outEP = { endpointNumber: 7 }
    d._closing = false

    await expect(d.send(new SendCommand('frame'))).resolves.toBe(false)
  })

  test('sendBluetoothPairedList delegates to send with SendBluetoothPairedList', async () => {
    const d = new DongleDriver() as any
    d.send = jest.fn(async () => true)

    await d.sendBluetoothPairedList('abc')

    expect(d.send).toHaveBeenCalledWith(expect.any(SendBluetoothPairedList))
  })

  test('sendGnssData delegates to send with SendGnssData', async () => {
    const d = new DongleDriver() as any
    d.send = jest.fn(async () => true)

    await d.sendGnssData('$GPGGA')

    expect(d.send).toHaveBeenCalledWith(expect.any(SendGnssData))
  })

  test('onOpened starts heartbeat once and sends post-open config', () => {
    const d = new DongleDriver() as any
    d.sendPostOpenConfig = jest.fn()
    d.send = jest.fn(async () => true)

    d.onOpened()
    d.onOpened()

    expect(d.sendPostOpenConfig).toHaveBeenCalledTimes(2)
    expect(d._heartbeatInterval).toBeTruthy()
  })

  test('onUnplugged clears phone hints and heartbeat interval', () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = PhoneType.CarPlay
    d._pendingModeHintFromBoxInfo = PhoneWorkMode.Android
    d._heartbeatInterval = setInterval(() => {}, 1000)

    d.onUnplugged()

    expect(d._lastPluggedPhoneType).toBeNull()
    expect(d._pendingModeHintFromBoxInfo).toBeNull()
    expect(d._heartbeatInterval).toBeNull()
  })

  test('onPlugged updates last phone type, reconciles modes and emits config-changed when needed', async () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')
    d.reconcileModes = jest.fn(async () => undefined)
    d._cfg = { lastPhoneWorkMode: PhoneWorkMode.CarPlay }

    await d.onPlugged({ phoneType: PhoneType.AndroidAuto })

    expect(d._lastPluggedPhoneType).toBe(PhoneType.AndroidAuto)
    expect(d.reconcileModes).toHaveBeenCalledWith('plugged')
    expect(d._cfg.lastPhoneWorkMode).toBe(PhoneWorkMode.Android)
    expect(emitSpy).toHaveBeenCalledWith('config-changed', {
      lastPhoneWorkMode: PhoneWorkMode.Android
    })
  })

  test('reconcileModes applies desired phone mode when plugged type implies change', async () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = PhoneType.AndroidAuto
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.applyAndroidWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.logAndroidWorkModeChange = jest.fn()

    await d.reconcileModes('plugged')

    expect(d.applyPhoneWorkMode).toHaveBeenCalledWith(PhoneWorkMode.Android)
    expect(d.applyAndroidWorkMode).not.toHaveBeenCalled()
  })

  test('reconcileModes applies desired android mode when plugged type implies change', async () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = PhoneType.AndroidAuto
    d._phoneWorkModeRuntime = PhoneWorkMode.Android
    d._androidWorkModeRuntime = AndroidWorkMode.Off
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.applyAndroidWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.logAndroidWorkModeChange = jest.fn()

    await d.reconcileModes('plugged')

    expect(d.applyAndroidWorkMode).toHaveBeenCalledWith(AndroidWorkMode.AndroidAuto)
  })

  test('readOneMessage returns null when device or endpoint is missing', async () => {
    const d = new DongleDriver() as any
    d._device = null
    d._inEP = null

    await expect(d.readOneMessage()).resolves.toBeNull()
  })

  test('start throws when initialise was not called', async () => {
    const d = new DongleDriver()

    await expect(
      d.start({ width: 800, height: 480, fps: 60, lastPhoneWorkMode: PhoneWorkMode.CarPlay } as any)
    ).rejects.toThrow(DriverStateError)
  })

  test('start returns early when device is not opened', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: false }
    d.send = jest.fn(async () => true)

    await d.start({ width: 800, height: 480, fps: 60, lastPhoneWorkMode: PhoneWorkMode.CarPlay })

    expect(d.send).not.toHaveBeenCalled()
  })

  test('start returns early when already started', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._started = true
    d.send = jest.fn(async () => true)

    await d.start({ width: 800, height: 480, fps: 60, lastPhoneWorkMode: PhoneWorkMode.CarPlay })

    expect(d.send).not.toHaveBeenCalled()
  })

  test('start stores config, sets initial modes and sends SendOpen', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)

    const cfg = {
      width: 800,
      height: 480,
      fps: 60,
      lastPhoneWorkMode: PhoneWorkMode.Android
    }

    await d.start(cfg as any)

    expect(d._started).toBe(true)
    expect(d._cfg).toBe(cfg)
    expect(d._phoneWorkModeRuntime).toBe(PhoneWorkMode.Android)
    expect(d._androidWorkModeRuntime).toBe(AndroidWorkMode.AndroidAuto)
    expect(d.send).toHaveBeenCalledWith(expect.any(SendOpen))
  })

  test('close returns early when nothing is active', async () => {
    const d = new DongleDriver()

    await expect(d.close()).resolves.toBeUndefined()
  })

  test('close resets logical state even when device close path is skipped', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: false }
    d._readerActive = true
    d._started = true
    d._heartbeatInterval = setInterval(() => {}, 1000)
    d._wifiConnectTimer = setTimeout(() => {}, 1000)
    d._inEP = {}
    d._outEP = {}
    d._ifaceNumber = 1
    d.errorCount = 3
    d._dongleFwVersion = '1.0.0'
    d._boxInfo = { productType: 'A15W' }
    d._lastDongleInfoEmitKey = 'x'
    d._postOpenConfigSent = true

    await d.close()

    expect(d._heartbeatInterval).toBeNull()
    expect(d._inEP).toBeNull()
    expect(d._outEP).toBeNull()
    expect(d._ifaceNumber).toBeNull()
    expect(d._started).toBe(false)
    expect(d._readerActive).toBe(false)
    expect(d.errorCount).toBe(0)
    expect(d._dongleFwVersion).toBeUndefined()
    expect(d._boxInfo).toBeUndefined()
    expect(d._lastDongleInfoEmitKey).toBe('')
    expect(d._postOpenConfigSent).toBe(false)
    expect(d._device).toBeNull()
  })

  test('initialise returns early when device already exists', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }

    const device = {
      opened: true,
      selectConfiguration: jest.fn(),
      claimInterface: jest.fn()
    }

    await d.initialise(device)

    expect(device.selectConfiguration).not.toHaveBeenCalled()
  })

  test('initialise throws when device is not opened', async () => {
    const d = new DongleDriver()

    await expect(d.initialise({ opened: false } as any)).rejects.toThrow('Device not opened')
  })

  test('initialise throws when configuration is missing', async () => {
    const d = new DongleDriver()

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: null,
      claimInterface: jest.fn()
    }

    await expect(d.initialise(device as any)).rejects.toThrow('Device has no configuration')
  })

  test('initialise throws when interface 0 is missing', async () => {
    const d = new DongleDriver()

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: { interfaces: [] },
      claimInterface: jest.fn()
    }

    await expect(d.initialise(device as any)).rejects.toThrow('No interface 0')
  })

  test('initialise throws when active alternate is missing', async () => {
    const d = new DongleDriver()

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: {
        interfaces: [{ interfaceNumber: 2, alternate: null }]
      },
      claimInterface: jest.fn(async () => undefined)
    }

    await expect(d.initialise(device as any)).rejects.toThrow('No active alternate on interface')
  })

  test('initialise throws when endpoints are missing', async () => {
    const d = new DongleDriver()

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: {
        interfaces: [
          {
            interfaceNumber: 2,
            alternate: { endpoints: [] }
          }
        ]
      },
      claimInterface: jest.fn(async () => undefined)
    }

    await expect(d.initialise(device as any)).rejects.toThrow('Endpoints missing')
  })

  test('initialise sets interface and endpoints and starts read loop once', async () => {
    const d = new DongleDriver() as any
    d.readLoop = jest.fn(async () => undefined)

    const inEp = { direction: 'in', endpointNumber: 1 }
    const outEp = { direction: 'out', endpointNumber: 2 }

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: {
        interfaces: [
          {
            interfaceNumber: 3,
            alternate: { endpoints: [inEp, outEp] }
          }
        ]
      },
      claimInterface: jest.fn(async () => undefined)
    }

    await d.initialise(device as any)

    expect(d._device).toBe(device)
    expect(d._ifaceNumber).toBe(3)
    expect(d._inEP).toBe(inEp)
    expect(d._outEP).toBe(outEp)
    expect(device.claimInterface).toHaveBeenCalledWith(3)
    expect(d.readLoop).toHaveBeenCalledTimes(1)
  })

  test('readOneMessage throws HeaderBuildError on empty header', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }
    d._device = {
      transferIn: jest.fn(async () => ({ data: null }))
    }

    await expect(d.readOneMessage()).rejects.toThrow(HeaderBuildError)
  })

  test('readOneMessage reads header-only message', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }

    const header = MessageHeader.asBuffer(MessageType.Open, 0)

    d._device = {
      transferIn: jest.fn(async () => ({
        data: new DataView(
          header.buffer.slice(header.byteOffset, header.byteOffset + header.byteLength)
        )
      }))
    }

    const msg = await d.readOneMessage()
    expect(msg).toBeInstanceOf(DongleReady)
  })

  test('readOneMessage reads payload message', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }

    const payload = Buffer.from('1.2.3\0', 'utf8')
    const header = MessageHeader.asBuffer(MessageType.SoftwareVersion, payload.length)

    d._device = {
      transferIn: jest
        .fn()
        .mockResolvedValueOnce({
          data: new DataView(
            header.buffer.slice(header.byteOffset, header.byteOffset + header.byteLength)
          )
        })
        .mockResolvedValueOnce({
          data: new DataView(
            payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength)
          )
        })
    }

    const msg = await d.readOneMessage()
    expect(msg).toBeInstanceOf(SoftwareVersion)
  })

  test('handleMessage stores software version and emits dongle info', async () => {
    const d = new DongleDriver() as any
    d.emitDongleInfoIfChanged = jest.fn()

    const msg = Object.create(SoftwareVersion.prototype)
    msg.version = '2.0.0'

    await d.handleMessage(msg)

    expect(d._dongleFwVersion).toBe('2.0.0')
    expect(d.emitDongleInfoIfChanged).toHaveBeenCalled()
  })

  test('handleMessage delegates BoxInfo to onBoxInfo and emits message', async () => {
    const d = new DongleDriver() as any
    d.onBoxInfo = jest.fn(async () => undefined)
    const emitSpy = jest.spyOn(d, 'emit')

    const msg = Object.create(BoxInfo.prototype)
    msg.settings = { productType: 'A15W' }

    await d.handleMessage(msg)

    expect(d.onBoxInfo).toHaveBeenCalledWith(msg)
    expect(emitSpy).toHaveBeenCalledWith('message', msg)
  })

  test('handleMessage emits message for VendorSessionInfo even when decrypt fails', async () => {
    const { decryptVendorSessionText } = jest.requireMock('@main/helpers/vendorSessionInfo')
    decryptVendorSessionText.mockRejectedValueOnce(new Error('boom'))

    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    const msg = Object.create(VendorSessionInfo.prototype)
    msg.raw = Buffer.from('abcd')

    await d.handleMessage(msg)

    expect(emitSpy).toHaveBeenCalledWith('message', msg)
  })

  test('handleMessage emits DongleReady message', async () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    const msg = Object.create(DongleReady.prototype)

    await d.handleMessage(msg)

    expect(emitSpy).toHaveBeenCalledWith('message', msg)
  })

  test('handleMessage routes Opened Unplugged and Plugged hooks', async () => {
    const d = new DongleDriver() as any
    d.onOpened = jest.fn()
    d.onUnplugged = jest.fn()
    d.onPlugged = jest.fn(async () => undefined)

    const opened = Object.create(Opened.prototype)
    const unplugged = Object.create(Unplugged.prototype)
    const plugged = Object.create(Plugged.prototype)

    await d.handleMessage(opened)
    await d.handleMessage(unplugged)
    await d.handleMessage(plugged)

    expect(d.onOpened).toHaveBeenCalled()
    expect(d.onUnplugged).toHaveBeenCalled()
    expect(d.onPlugged).toHaveBeenCalledWith(plugged)
  })

  test('handleMessage tolerates BluetoothPeerConnected no-op path', async () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')
    const msg = Object.create(BluetoothPeerConnected.prototype)

    await d.handleMessage(msg)

    expect(emitSpy).toHaveBeenCalledWith('message', msg)
  })

  test('setPendingStartupConnectTarget stores trimmed btMac and phoneWorkMode', () => {
    const d = new DongleDriver() as any

    d.setPendingStartupConnectTarget({
      btMac: '  AA:BB:CC:DD:EE:FF  ',
      phoneWorkMode: PhoneWorkMode.Android
    })

    expect(d._pendingStartupConnectTarget).toEqual({
      btMac: 'AA:BB:CC:DD:EE:FF',
      phoneWorkMode: PhoneWorkMode.Android
    })
  })

  test('setPendingStartupConnectTarget clears target for empty btMac', () => {
    const d = new DongleDriver() as any
    d._pendingStartupConnectTarget = { btMac: 'x', phoneWorkMode: PhoneWorkMode.CarPlay }

    d.setPendingStartupConnectTarget({
      btMac: '   ',
      phoneWorkMode: PhoneWorkMode.Android
    })

    expect(d._pendingStartupConnectTarget).toBeNull()
  })

  test('setPendingStartupConnectTarget clears target when called with null', () => {
    const d = new DongleDriver() as any
    d._pendingStartupConnectTarget = { btMac: 'x', phoneWorkMode: PhoneWorkMode.CarPlay }

    d.setPendingStartupConnectTarget(null)

    expect(d._pendingStartupConnectTarget).toBeNull()
  })

  test('clearPendingStartupConnectTarget clears pending target', () => {
    const d = new DongleDriver() as any
    d._pendingStartupConnectTarget = { btMac: 'x', phoneWorkMode: PhoneWorkMode.CarPlay }

    d.clearPendingStartupConnectTarget()

    expect(d._pendingStartupConnectTarget).toBeNull()
  })

  test('isBenignUsbShutdownError detects benign usb shutdown messages', () => {
    const d = new DongleDriver() as any

    expect(d.isBenignUsbShutdownError(new Error('LIBUSB_ERROR_NO_DEVICE'))).toBe(true)
    expect(d.isBenignUsbShutdownError(new Error('device has been disconnected'))).toBe(true)
    expect(d.isBenignUsbShutdownError(new Error('No such device'))).toBe(true)
    expect(d.isBenignUsbShutdownError(new Error('some other error'))).toBe(false)
  })

  test('tryResetUnderlyingUsbDevice returns false when no raw device exists', async () => {
    const d = new DongleDriver() as any

    await expect(d.tryResetUnderlyingUsbDevice({})).resolves.toBe(false)
  })

  test('tryResetUnderlyingUsbDevice returns false when reset is not a function', async () => {
    const d = new DongleDriver() as any

    await expect(d.tryResetUnderlyingUsbDevice({ device: {} })).resolves.toBe(false)
  })

  test('tryResetUnderlyingUsbDevice returns true when callback reset succeeds', async () => {
    const d = new DongleDriver() as any
    const raw = {
      reset: jest.fn((cb) => cb(null))
    }

    await expect(d.tryResetUnderlyingUsbDevice({ device: raw })).resolves.toBe(true)
  })

  test('tryResetUnderlyingUsbDevice returns false when callback reset fails', async () => {
    const d = new DongleDriver() as any
    const raw = {
      reset: jest.fn((cb) => cb(new Error('boom')))
    }

    await expect(d.tryResetUnderlyingUsbDevice({ device: raw })).resolves.toBe(false)
  })

  test('applyPhoneWorkMode no-ops when mode is unchanged', async () => {
    const d = new DongleDriver() as any
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay

    await d.applyPhoneWorkMode(PhoneWorkMode.CarPlay)

    expect(d._phoneWorkModeRuntime).toBe(PhoneWorkMode.CarPlay)
  })

  test('applyPhoneWorkMode no-ops when mode switch was too recent', async () => {
    const d = new DongleDriver() as any
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._lastModeSwitchAt = Date.now()
    d.send = jest.fn()
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._device = { opened: true }

    await d.applyPhoneWorkMode(PhoneWorkMode.Android)

    expect(d.send).not.toHaveBeenCalled()
  })

  test('applyPhoneWorkMode updates mode and sends disconnect + open', async () => {
    const d = new DongleDriver() as any
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._lastModeSwitchAt = 0
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._device = { opened: true }
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)

    await d.applyPhoneWorkMode(PhoneWorkMode.Android)

    expect(d._phoneWorkModeRuntime).toBe(PhoneWorkMode.Android)
    expect(d.send).toHaveBeenCalledTimes(2)
    expect(d.send.mock.calls[0][0]).toBeInstanceOf(SendDisconnectPhone)
    expect(d.send.mock.calls[1][0]).toBeInstanceOf(SendOpen)
  })

  test('onBoxInfo flips phone mode on explicit MDLinkType mismatch signal', async () => {
    const d = new DongleDriver() as any
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._phoneWorkModeRuntime = PhoneWorkMode.Android
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.emitDongleInfoIfChanged = jest.fn()

    const msg = Object.create(BoxInfo.prototype)
    msg.settings = { MDLinkType: 'RiddleLinktype_UNKNOWN?' }

    await d.onBoxInfo(msg)

    expect(d._boxInfo).toEqual({ MDLinkType: 'RiddleLinktype_UNKNOWN?' })
    expect(d.emitDongleInfoIfChanged).toHaveBeenCalledTimes(2)
    expect(d.logPhoneWorkModeChange).toHaveBeenCalled()
    expect(d.applyPhoneWorkMode).toHaveBeenCalledWith(PhoneWorkMode.CarPlay)
  })

  test('onBoxInfo also handles typo UNKOWN mismatch signal', async () => {
    const d = new DongleDriver() as any
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.emitDongleInfoIfChanged = jest.fn()

    const msg = Object.create(BoxInfo.prototype)
    msg.settings = { MDLinkType: 'RiddleLinktype_UNKOWN?' }

    await d.onBoxInfo(msg)

    expect(d.applyPhoneWorkMode).toHaveBeenCalledWith(PhoneWorkMode.Android)
  })

  test('sendPostOpenConfig returns early when already sent', async () => {
    const d = new DongleDriver() as any
    d._postOpenConfigSent = true
    d.send = jest.fn()

    await d.sendPostOpenConfig()

    expect(d.send).not.toHaveBeenCalled()
  })

  test('sendPostOpenConfig returns early when config is missing', async () => {
    const d = new DongleDriver() as any
    d._cfg = null
    d.send = jest.fn()

    await d.sendPostOpenConfig()

    expect(d.send).not.toHaveBeenCalled()
  })

  test('sendPostOpenConfig returns early when driver is closing', async () => {
    const d = new DongleDriver() as any
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._closing = true
    d._device = { opened: true }
    d.send = jest.fn()

    await d.sendPostOpenConfig()

    expect(d.send).not.toHaveBeenCalled()
  })

  test('sendPostOpenConfig sends setup messages and schedules wifi connect', async () => {
    const d = new DongleDriver() as any
    d._cfg = {
      width: 800,
      height: 480,
      fps: 60,
      carName: 'Car',
      oemName: 'OEM',
      micType: MicType.PhoneMic,
      nightMode: true,
      hand: 1,
      wifiType: '5ghz',
      audioTransferMode: true,
      projectionSafeAreaTop: 0,
      projectionSafeAreaBottom: 0,
      projectionSafeAreaLeft: 0,
      projectionSafeAreaRight: 0,
      projectionSafeAreaDrawOutside: false
    }
    d._device = { opened: true }
    d._closing = false
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.scheduleWifiConnect = jest.fn()

    await d.sendPostOpenConfig()

    expect(d.send).toHaveBeenCalled()
    expect(d.scheduleWifiConnect).toHaveBeenCalledWith(150)
    expect(d._postOpenConfigSent).toBe(true)
  })

  test('sendPostOpenConfig sends targeted auto-connect when pending target exists', async () => {
    const d = new DongleDriver() as any
    d._cfg = {
      width: 800,
      height: 480,
      fps: 60,
      carName: 'Car',
      oemName: 'OEM',
      micType: MicType.PhoneMic,
      nightMode: false,
      hand: 0,
      wifiType: '2.4ghz',
      audioTransferMode: false,
      projectionSafeAreaTop: 0,
      projectionSafeAreaBottom: 0,
      projectionSafeAreaLeft: 0,
      projectionSafeAreaRight: 0,
      projectionSafeAreaDrawOutside: false
    }
    d._device = { opened: true }
    d._closing = false
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d._pendingStartupConnectTarget = {
      btMac: 'AA:BB:CC:DD:EE:FF',
      phoneWorkMode: PhoneWorkMode.Android
    }
    d._wifiConnectTimer = setTimeout(() => {}, 1000)
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    const emitSpy = jest.spyOn(d, 'emit')

    await d.sendPostOpenConfig()

    expect(d.send).toHaveBeenCalledWith(expect.any(SendAutoConnectByBtAddress))
    expect(emitSpy).toHaveBeenCalledWith('targeted-connect-dispatched', {
      btMac: 'AA:BB:CC:DD:EE:FF',
      phoneWorkMode: PhoneWorkMode.Android
    })
    expect(d._pendingStartupConnectTarget).toBeNull()
  })

  test('sendPostOpenConfig logs targeted auto-connect in DEBUG mode', async () => {
    const debugSpy = jest.spyOn(console, 'debug').mockImplementation(() => {})

    jest.resetModules()

    await jest.isolateModulesAsync(async () => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      const {
        DongleDriver,
        AndroidWorkMode
      } = require('@main/services/projection/driver/DongleDriver')
      const { PhoneWorkMode, MicType } = require('@shared/types')

      const d = new DongleDriver() as any
      d._cfg = {
        width: 800,
        height: 480,
        fps: 60,
        carName: 'Car',
        oemName: 'OEM',
        micType: MicType.PhoneMic,
        nightMode: false,
        hand: 0,
        wifiType: '2.4ghz',
        audioTransferMode: false,
        projectionSafeAreaTop: 0,
        projectionSafeAreaBottom: 0,
        projectionSafeAreaLeft: 0,
        projectionSafeAreaRight: 0,
        projectionSafeAreaDrawOutside: false
      }
      d._device = { opened: true }
      d._closing = false
      d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
      d._pendingStartupConnectTarget = {
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.Android
      }
      d.send = jest.fn(async () => true)
      d.sleep = jest.fn(async () => undefined)

      await d.sendPostOpenConfig()
    })

    expect(debugSpy).toHaveBeenCalledWith(
      '[DongleDriver] sendPostOpenConfig uses targeted auto-connect',
      {
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.Android
      }
    )

    debugSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
  })

  test('reconcileModes uses pending mode hint from boxinfo without touching android mode', async () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = null
    d._pendingModeHintFromBoxInfo = PhoneWorkMode.Android
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._androidWorkModeRuntime = AndroidWorkMode.Search

    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.applyAndroidWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.logAndroidWorkModeChange = jest.fn()

    await d.reconcileModes('boxinfo')

    expect(d.applyPhoneWorkMode).toHaveBeenCalledWith(PhoneWorkMode.Android)
    expect(d.applyAndroidWorkMode).not.toHaveBeenCalled()
  })

  test('readLoop returns immediately when reader is already active', async () => {
    const d = new DongleDriver() as any
    d._readerActive = true

    await d.readLoop()

    expect(d._readerActive).toBe(true)
  })

  test('readLoop closes and emits failure when max error count is reached', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false
    d.errorCount = 5
    d.close = jest.fn(async () => undefined)
    const emitSpy = jest.spyOn(d, 'emit')

    await d.readLoop()

    expect(d.close).toHaveBeenCalled()
    expect(emitSpy).toHaveBeenCalledWith('failure')
    expect(d._readerActive).toBe(false)
  })

  test('readLoop continues when readOneMessage returns null', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false

    d.readOneMessage = jest
      .fn()
      .mockResolvedValueOnce(null)
      .mockImplementationOnce(async () => {
        d._closing = true
        return null
      })

    d.handleMessage = jest.fn(async () => undefined)

    await d.readLoop()

    expect(d.handleMessage).not.toHaveBeenCalled()
    expect(d._readerActive).toBe(false)
  })

  test('readLoop resets errorCount to 0 after a successful message', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false
    d.errorCount = 3

    const msg = Object.create(DongleReady.prototype)

    d.readOneMessage = jest
      .fn()
      .mockResolvedValueOnce(msg)
      .mockImplementationOnce(async () => {
        d._closing = true
        return null
      })

    d.handleMessage = jest.fn(async () => undefined)

    await d.readLoop()

    expect(d.handleMessage).toHaveBeenCalledWith(msg)
    expect(d.errorCount).toBe(0)
  })

  test('readLoop warns on HeaderBuildError and increments errorCount', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false

    d.readOneMessage = jest
      .fn()
      .mockRejectedValueOnce(new HeaderBuildError('bad header'))
      .mockImplementationOnce(async () => {
        d._closing = true
        return null
      })

    await d.readLoop()

    expect(warnSpy).toHaveBeenCalledWith('[DongleDriver] HeaderBuildError', 'bad header')
    expect(d.errorCount).toBe(1)

    warnSpy.mockRestore()
  })

  test('readLoop logs non-header errors and increments errorCount', async () => {
    const errorSpy = jest.spyOn(console, 'error').mockImplementation(() => {})
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false

    d.readOneMessage = jest
      .fn()
      .mockRejectedValueOnce(new Error('boom'))
      .mockImplementationOnce(async () => {
        d._closing = true
        return null
      })

    d.isBenignUsbShutdownError = jest.fn(() => false)

    await d.readLoop()

    expect(errorSpy).toHaveBeenCalledWith('[DongleDriver] readLoop error', expect.any(Error))
    expect(d.errorCount).toBe(1)

    errorSpy.mockRestore()
  })

  test('readLoop breaks on benign usb shutdown error', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false

    d.readOneMessage = jest.fn().mockRejectedValueOnce(new Error('LIBUSB_ERROR_NO_DEVICE'))
    d.isBenignUsbShutdownError = jest.fn(() => true)

    await d.readLoop()

    expect(d.errorCount).toBe(0)
    expect(d._readerActive).toBe(false)
  })

  test('close retries device.close after pending request and underlying reset success', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest
        .fn()
        .mockRejectedValueOnce(new Error('pending request'))
        .mockResolvedValueOnce(undefined)
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.tryResetUnderlyingUsbDevice = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.close(): pending request -> trying underlying usb reset()'
    )
    expect(d.tryResetUnderlyingUsbDevice).toHaveBeenCalledWith(dev)
    expect(dev.close).toHaveBeenCalledTimes(2)

    warnSpy.mockRestore()
  })

  test('close keeps device reference when pending request persists on second close', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => {
        throw new Error('pending request')
      })
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.tryResetUnderlyingUsbDevice = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.close(): pending request did not resolve before deadline'
    )
    expect(d._device).toBe(dev)

    warnSpy.mockRestore()
  })

  test('close warns when second close after pending request fails with another error', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest
        .fn()
        .mockRejectedValueOnce(new Error('pending request'))
        .mockRejectedValueOnce(new Error('other close error'))
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.tryResetUnderlyingUsbDevice = jest.fn(async () => false)
    d.sleep = jest.fn(async () => undefined)
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith('[DongleDriver] device.close() failed', expect.any(Error))

    warnSpy.mockRestore()
  })

  test('close warns on outer close error', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {}
    Object.defineProperty(dev, 'opened', {
      get() {
        throw new Error('outer close boom')
      },
      configurable: true
    })

    d._device = dev
    d._readerActive = true
    d._started = true

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith('[DongleDriver] close() outer error', expect.any(Error))

    warnSpy.mockRestore()
  })

  test('logPhoneWorkModeChange and logAndroidWorkModeChange include extra text', () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
    const d = new DongleDriver() as any

    d.logPhoneWorkModeChange(
      'test-reason',
      PhoneWorkMode.CarPlay,
      PhoneWorkMode.Android,
      'extra-info'
    )

    d.logAndroidWorkModeChange(
      'test-reason',
      AndroidWorkMode.Off,
      AndroidWorkMode.AndroidAuto,
      'extra-info'
    )

    expect(logSpy).toHaveBeenCalledWith(
      '[DongleDriver] phone work mode change | reason=test-reason | from=CarPlay | to=Android | extra-info'
    )
    expect(logSpy).toHaveBeenCalledWith(
      '[DongleDriver] android work mode change | reason=test-reason | from=Off | to=AndroidAuto | extra-info'
    )

    logSpy.mockRestore()
  })

  test('sleep resolves after timeout and waitForReaderStop polls until reader stops', async () => {
    const d = new DongleDriver() as any

    const sleepPromise = d.sleep(25)
    jest.advanceTimersByTime(25)
    await expect(sleepPromise).resolves.toBeUndefined()

    const sleepSpy = jest.spyOn(d, 'sleep')
    d._readerActive = true

    const waitPromise = d.waitForReaderStop(100)

    expect(sleepSpy).toHaveBeenCalledWith(10)

    d._readerActive = false
    jest.advanceTimersByTime(10)

    await expect(waitPromise).resolves.toBeUndefined()

    sleepSpy.mockRestore()
  })

  test('emitDongleInfoIfChanged falls back to String(box) when JSON.stringify throws', () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    const badBox = {
      toString: () => 'bad-box'
    } as any
    badBox.self = badBox

    d._dongleFwVersion = '1.0.0'
    d._boxInfo = badBox

    d.emitDongleInfoIfChanged()

    expect(emitSpy).toHaveBeenCalledWith('dongle-info', {
      dongleFwVersion: '1.0.0',
      boxInfo: badBox
    })
    expect(d._lastDongleInfoEmitKey).toBe('1.0.0||bad-box')
  })

  test('handleMessage logs decrypted VendorSessionInfo in DEBUG mode', async () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    await jest.isolateModulesAsync(async () => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      jest.doMock('@main/helpers/vendorSessionInfo', () => ({
        decryptVendorSessionText: jest.fn(async () => 'decrypted-session')
      }))

      const { DongleDriver } = require('@main/services/projection/driver/DongleDriver')
      const { VendorSessionInfo } = require('@main/services/projection/messages/readable')

      const d = new DongleDriver() as any
      const emitSpy = jest.spyOn(d, 'emit')

      const msg = Object.create(VendorSessionInfo.prototype)
      msg.raw = Buffer.from('abcd')

      await d.handleMessage(msg)

      expect(logSpy).toHaveBeenCalledWith('[DongleDriver] VendorSessionInfo decrypted-session')
      expect(emitSpy).toHaveBeenCalledWith('message', msg)
    })

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
    jest.dontMock('@main/helpers/vendorSessionInfo')
  })

  test('reconcileModes does nothing when no plugged type and no pending boxinfo hint exist', async () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = null
    d._pendingModeHintFromBoxInfo = null
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto

    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.applyAndroidWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.logAndroidWorkModeChange = jest.fn()

    await d.reconcileModes('boxinfo')

    expect(d.applyPhoneWorkMode).not.toHaveBeenCalled()
    expect(d.applyAndroidWorkMode).not.toHaveBeenCalled()
  })

  test('readLoop leaves errorCount unchanged when it is already zero after successful message', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d._closing = false
    d.errorCount = 0

    const msg = Object.create(DongleReady.prototype)

    d.readOneMessage = jest
      .fn()
      .mockResolvedValueOnce(msg)
      .mockImplementationOnce(async () => {
        d._closing = true
        return null
      })

    d.handleMessage = jest.fn(async () => undefined)

    await d.readLoop()

    expect(d.handleMessage).toHaveBeenCalledWith(msg)
    expect(d.errorCount).toBe(0)
  })

  test('start defaults phone work mode to CarPlay when lastPhoneWorkMode is not Android', async () => {
    const d = new DongleDriver() as any
    d._device = { opened: true }
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)

    const cfg = {
      width: 800,
      height: 480,
      fps: 60,
      lastPhoneWorkMode: PhoneWorkMode.CarPlay
    }

    await d.start(cfg as any)

    expect(d._phoneWorkModeRuntime).toBe(PhoneWorkMode.CarPlay)
    expect(d.send).toHaveBeenCalledWith(expect.any(SendOpen))
  })

  test('close returns early when a close promise already exists', async () => {
    const d = new DongleDriver() as any
    const existing = Promise.resolve()
    d._closePromise = existing

    const result = d.close()

    expect(d._closePromise).toBe(existing)
    await expect(result).resolves.toBeUndefined()
  })

  test('close ignores darwin reset errors for LIBUSB_ERROR_NOT_FOUND without warning', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const originalPlatform = process.platform

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })

    const d = new DongleDriver() as any
    const dev = {
      opened: true,
      reset: jest.fn(async () => {
        throw new Error('LIBUSB_ERROR_NOT_FOUND')
      }),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(dev.reset).toHaveBeenCalled()
    expect(warnSpy).not.toHaveBeenCalledWith(
      '[DongleDriver] device.reset() failed (ignored)',
      expect.anything()
    )

    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
      configurable: true
    })
    warnSpy.mockRestore()
  })

  test('close handles non-Error close failure message on first close attempt', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => {
        throw 'plain string close error'
      })
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.close() failed',
      'plain string close error'
    )

    warnSpy.mockRestore()
  })

  test('close handles non-Error second close failure after pending request', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const d = new DongleDriver() as any

    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest
        .fn()
        .mockRejectedValueOnce(new Error('pending request'))
        .mockRejectedValueOnce('second plain string error')
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.tryResetUnderlyingUsbDevice = jest.fn(async () => false)
    d.waitForReaderStop = jest.fn(async () => undefined)
    d.sleep = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.close() failed',
      'second plain string error'
    )

    warnSpy.mockRestore()
  })

  test('handleMessage logs decrypted VendorSessionInfo when DEBUG is true', async () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    await jest.isolateModulesAsync(async () => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      jest.doMock('@main/helpers/vendorSessionInfo', () => ({
        decryptVendorSessionText: jest.fn(async () => 'decrypted-session')
      }))

      const { DongleDriver } = require('@main/services/projection/driver/DongleDriver')
      const { VendorSessionInfo } = require('@main/services/projection/messages/readable')

      const d = new DongleDriver() as any
      const emitSpy = jest.spyOn(d, 'emit')

      const msg = Object.create(VendorSessionInfo.prototype)
      msg.raw = Buffer.from('abcd')

      await d.handleMessage(msg)

      expect(logSpy).toHaveBeenCalledWith('[DongleDriver] VendorSessionInfo decrypted-session')
      expect(emitSpy).toHaveBeenCalledWith('message', msg)
    })

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
    jest.dontMock('@main/helpers/vendorSessionInfo')
  })

  test('sendPostOpenConfig falls back to carName and uses DongleMic route command', async () => {
    const d = new DongleDriver() as any
    d._cfg = {
      width: 800,
      height: 480,
      fps: 60,
      carName: 'FallbackCar',
      oemName: '   ',
      micType: MicType.DongleMic,
      nightMode: false,
      hand: 0,
      wifiType: '2.4ghz',
      audioTransferMode: false,
      projectionSafeAreaTop: 0,
      projectionSafeAreaBottom: 0,
      projectionSafeAreaLeft: 0,
      projectionSafeAreaRight: 0,
      projectionSafeAreaDrawOutside: false
    }
    d._device = { opened: true }
    d._closing = false
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.scheduleWifiConnect = jest.fn()

    await d.sendPostOpenConfig()

    expect(d.send).toHaveBeenCalledWith(expect.any(SendString))

    const sent = d.send.mock.calls.map((c: any[]) => c[0])
    const labelMsg = sent.find((m: any) => m instanceof SendString)
    const micMsg = sent.find(
      (m: any) => m instanceof SendCommand && m.value === CommandMapping.boxMici2s
    )

    expect(labelMsg).toBeInstanceOf(SendString)
    expect(micMsg).toBeInstanceOf(SendCommand)
  })

  test('sendPostOpenConfig uses default mic route command when micType is neither DongleMic nor PhoneMic', async () => {
    const d = new DongleDriver() as any
    d._cfg = {
      width: 800,
      height: 480,
      fps: 60,
      carName: 'Car',
      oemName: 'OEM',
      micType: 999,
      nightMode: false,
      hand: 0,
      wifiType: '2.4ghz',
      audioTransferMode: false,
      projectionSafeAreaTop: 0,
      projectionSafeAreaBottom: 0,
      projectionSafeAreaLeft: 0,
      projectionSafeAreaRight: 0,
      projectionSafeAreaDrawOutside: false
    }
    d._device = { opened: true }
    d._closing = false
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.scheduleWifiConnect = jest.fn()

    await d.sendPostOpenConfig()

    expect(
      d.send.mock.calls.some(
        (c: any[]) => c[0] instanceof SendCommand && c[0].value === CommandMapping.mic
      )
    ).toBe(true)
  })

  test('onUnplugged keeps heartbeat null when no heartbeat interval exists', () => {
    const d = new DongleDriver() as any
    d._lastPluggedPhoneType = PhoneType.CarPlay
    d._pendingModeHintFromBoxInfo = PhoneWorkMode.Android
    d._heartbeatInterval = null

    d.onUnplugged()

    expect(d._lastPluggedPhoneType).toBeNull()
    expect(d._pendingModeHintFromBoxInfo).toBeNull()
    expect(d._heartbeatInterval).toBeNull()
  })

  test('onPlugged does not emit config-changed when lastPhoneWorkMode is already correct', async () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')
    d.reconcileModes = jest.fn(async () => undefined)
    d._cfg = { lastPhoneWorkMode: PhoneWorkMode.Android }

    await d.onPlugged({ phoneType: PhoneType.AndroidAuto })

    expect(d._cfg.lastPhoneWorkMode).toBe(PhoneWorkMode.Android)
    expect(emitSpy).not.toHaveBeenCalledWith('config-changed', expect.anything())
  })

  test('onBoxInfo does not flip mode on mismatch when config is missing', async () => {
    const d = new DongleDriver() as any
    d._cfg = null
    d._phoneWorkModeRuntime = PhoneWorkMode.Android
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.emitDongleInfoIfChanged = jest.fn()

    const msg = Object.create(BoxInfo.prototype)
    msg.settings = { MDLinkType: 'RiddleLinktype_UNKNOWN?' }

    await d.onBoxInfo(msg)

    expect(d._boxInfo).toEqual({ MDLinkType: 'RiddleLinktype_UNKNOWN?' })
    expect(d.emitDongleInfoIfChanged).toHaveBeenCalledTimes(2)
    expect(d.logPhoneWorkModeChange).not.toHaveBeenCalled()
    expect(d.applyPhoneWorkMode).not.toHaveBeenCalled()
  })

  test('close warns when darwin device.reset fails with non-benign error', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const originalPlatform = process.platform

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })

    const d = new DongleDriver() as any
    const dev = {
      opened: true,
      reset: jest.fn(async () => {
        throw new Error('reset exploded')
      }),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.reset() failed (ignored)',
      expect.any(Error)
    )

    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
      configurable: true
    })
    warnSpy.mockRestore()
  })

  test('readOneMessage throws when payload transfer returns no extra data', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }

    const payloadLength = 4
    const header = MessageHeader.asBuffer(MessageType.SoftwareVersion, payloadLength)

    d._device = {
      transferIn: jest
        .fn()
        .mockResolvedValueOnce({
          data: new DataView(
            header.buffer.slice(header.byteOffset, header.byteOffset + header.byteLength)
          )
        })
        .mockResolvedValueOnce({
          data: null
        })
    }

    await expect(d.readOneMessage()).rejects.toThrow('Failed to read extra data')
  })

  test('handleMessage logs decrypted VendorSessionInfo when DEBUG is true', async () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    await jest.isolateModulesAsync(async () => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      jest.doMock('@main/helpers/vendorSessionInfo', () => ({
        decryptVendorSessionText: jest.fn(async () => 'decrypted-session')
      }))

      const { DongleDriver } = require('@main/services/projection/driver/DongleDriver')
      const { VendorSessionInfo } = require('@main/services/projection/messages/readable')

      const d = new DongleDriver() as any
      const emitSpy = jest.spyOn(d, 'emit')

      const msg = Object.create(VendorSessionInfo.prototype)
      msg.raw = Buffer.from('abcd')

      await d.handleMessage(msg)

      expect(logSpy).toHaveBeenCalledWith('[DongleDriver] VendorSessionInfo decrypted-session')
      expect(emitSpy).toHaveBeenCalledWith('message', msg)
    })

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
    jest.dontMock('@main/helpers/vendorSessionInfo')
  })

  test('sendPostOpenConfig falls back to carName when oemName is undefined and uses PhoneMic route', async () => {
    const d = new DongleDriver() as any
    d._cfg = {
      width: 800,
      height: 480,
      fps: 60,
      carName: 'FallbackCar',
      oemName: undefined,
      micType: MicType.PhoneMic,
      nightMode: false,
      hand: 0,
      wifiType: '2.4ghz',
      audioTransferMode: false,
      projectionSafeAreaTop: 0,
      projectionSafeAreaBottom: 0,
      projectionSafeAreaLeft: 0,
      projectionSafeAreaRight: 0,
      projectionSafeAreaDrawOutside: false
    }
    d._device = { opened: true }
    d._closing = false
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)
    d.scheduleWifiConnect = jest.fn()

    await d.sendPostOpenConfig()

    const sent = d.send.mock.calls.map((c: any[]) => c[0])

    expect(sent.some((m: any) => m instanceof SendString)).toBe(true)
    expect(
      sent.some((m: any) => m instanceof SendCommand && m.value === CommandMapping.phoneMic)
    ).toBe(true)
  })

  test('onPlugged skips config-changed update when config is missing', async () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')
    d.reconcileModes = jest.fn(async () => undefined)
    d._cfg = null

    await d.onPlugged({ phoneType: PhoneType.AndroidAuto })

    expect(d._lastPluggedPhoneType).toBe(PhoneType.AndroidAuto)
    expect(d.reconcileModes).toHaveBeenCalledWith('plugged')
    expect(emitSpy).not.toHaveBeenCalledWith('config-changed', expect.anything())
  })

  test('onBoxInfo ignores empty MDLinkType without flipping modes', async () => {
    const d = new DongleDriver() as any
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._phoneWorkModeRuntime = PhoneWorkMode.Android
    d.applyPhoneWorkMode = jest.fn(async () => undefined)
    d.logPhoneWorkModeChange = jest.fn()
    d.emitDongleInfoIfChanged = jest.fn()

    const msg = Object.create(BoxInfo.prototype)
    msg.settings = {}

    await d.onBoxInfo(msg)

    expect(d._boxInfo).toEqual({})
    expect(d.emitDongleInfoIfChanged).toHaveBeenCalledTimes(2)
    expect(d.logPhoneWorkModeChange).not.toHaveBeenCalled()
    expect(d.applyPhoneWorkMode).not.toHaveBeenCalled()
  })

  test('close does not call device.reset on non-darwin platforms', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const originalPlatform = process.platform

    Object.defineProperty(process, 'platform', {
      value: 'linux',
      configurable: true
    })

    const d = new DongleDriver() as any
    const dev = {
      opened: true,
      reset: jest.fn(async () => undefined),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(dev.reset).not.toHaveBeenCalled()

    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
      configurable: true
    })
    warnSpy.mockRestore()
  })

  test('isBenignUsbShutdownError also handles non-Error values', () => {
    const d = new DongleDriver() as any

    expect(d.isBenignUsbShutdownError('No such device')).toBe(true)
    expect(d.isBenignUsbShutdownError('plain unrelated error')).toBe(false)
  })

  test('emitDongleInfoIfChanged works when box info is undefined', () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    d._dongleFwVersion = '1.2.3'
    d._boxInfo = undefined

    d.emitDongleInfoIfChanged()

    expect(emitSpy).toHaveBeenCalledWith('dongle-info', {
      dongleFwVersion: '1.2.3',
      boxInfo: undefined
    })
    expect(d._lastDongleInfoEmitKey).toBe('1.2.3||')
  })

  test('initialise does not start readLoop again when reader is already active', async () => {
    const d = new DongleDriver() as any
    d._readerActive = true
    d.readLoop = jest.fn(async () => undefined)

    const inEp = { direction: 'in', endpointNumber: 1 }
    const outEp = { direction: 'out', endpointNumber: 2 }

    const device = {
      opened: true,
      selectConfiguration: jest.fn(async () => undefined),
      configuration: {
        interfaces: [
          {
            interfaceNumber: 3,
            alternate: { endpoints: [inEp, outEp] }
          }
        ]
      },
      claimInterface: jest.fn(async () => undefined)
    }

    await d.initialise(device as any)

    expect(d.readLoop).not.toHaveBeenCalled()
  })

  test('readOneMessage returns null when closing after header read', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }

    const header = MessageHeader.asBuffer(MessageType.Open, 0)

    d._device = {
      transferIn: jest.fn(async () => {
        d._closing = true
        return {
          data: new DataView(
            header.buffer.slice(header.byteOffset, header.byteOffset + header.byteLength)
          )
        }
      })
    }

    await expect(d.readOneMessage()).resolves.toBeNull()
  })

  test('readOneMessage returns null when closing after payload read', async () => {
    const d = new DongleDriver() as any
    d._inEP = { endpointNumber: 7 }

    const payload = Buffer.from('1.2.3\0', 'utf8')
    const header = MessageHeader.asBuffer(MessageType.SoftwareVersion, payload.length)

    d._device = {
      transferIn: jest
        .fn()
        .mockResolvedValueOnce({
          data: new DataView(
            header.buffer.slice(header.byteOffset, header.byteOffset + header.byteLength)
          )
        })
        .mockImplementationOnce(async () => {
          d._closing = true
          return {
            data: new DataView(
              payload.buffer.slice(payload.byteOffset, payload.byteOffset + payload.byteLength)
            )
          }
        })
    }

    await expect(d.readOneMessage()).resolves.toBeNull()
  })

  test('handleMessage logs decrypted VendorSessionInfo when DEBUG is true', async () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    await jest.isolateModulesAsync(async () => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      jest.doMock('@main/helpers/vendorSessionInfo', () => ({
        decryptVendorSessionText: jest.fn(async () => 'decrypted-session')
      }))

      const { DongleDriver } = require('@main/services/projection/driver/DongleDriver')
      const { VendorSessionInfo } = require('@main/services/projection/messages/readable')

      const d = new DongleDriver() as any
      const emitSpy = jest.spyOn(d, 'emit')

      const msg = Object.create(VendorSessionInfo.prototype)
      msg.raw = Buffer.from('abcd')

      await d.handleMessage(msg)

      expect(logSpy).toHaveBeenCalledWith('[DongleDriver] VendorSessionInfo decrypted-session')
      expect(emitSpy).toHaveBeenCalledWith('message', msg)
    })

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
    jest.dontMock('@main/helpers/vendorSessionInfo')
  })

  test('close warns when darwin reset fails with non-Error value', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const originalPlatform = process.platform

    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })

    const d = new DongleDriver() as any
    const dev = {
      opened: true,
      reset: jest.fn(async () => {
        throw 'plain reset failure'
      }),
      releaseInterface: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }

    d._device = dev
    d._ifaceNumber = 1
    d._readerActive = true
    d._started = true
    d.waitForReaderStop = jest.fn(async () => undefined)

    await d.close()

    expect(warnSpy).toHaveBeenCalledWith(
      '[DongleDriver] device.reset() failed (ignored)',
      'plain reset failure'
    )

    Object.defineProperty(process, 'platform', {
      value: originalPlatform,
      configurable: true
    })
    warnSpy.mockRestore()
  })

  test('tryResetUnderlyingUsbDevice returns false when candidate raw device is not an object', async () => {
    const d = new DongleDriver() as any

    await expect(d.tryResetUnderlyingUsbDevice({ device: 'not-an-object' })).resolves.toBe(false)
  })

  test('logPhoneWorkModeChange and logAndroidWorkModeChange omit extra separator when no extra is provided', () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
    const d = new DongleDriver() as any

    d.logPhoneWorkModeChange('reason-x', PhoneWorkMode.CarPlay, PhoneWorkMode.Android)
    d.logAndroidWorkModeChange('reason-y', AndroidWorkMode.Off, AndroidWorkMode.AndroidAuto)

    expect(logSpy).toHaveBeenCalledWith(
      '[DongleDriver] phone work mode change | reason=reason-x | from=CarPlay | to=Android'
    )
    expect(logSpy).toHaveBeenCalledWith(
      '[DongleDriver] android work mode change | reason=reason-y | from=Off | to=AndroidAuto'
    )

    logSpy.mockRestore()
  })

  test('applyPhoneWorkMode returns early when config is missing', async () => {
    const d = new DongleDriver() as any
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._lastModeSwitchAt = 0
    d._cfg = null
    d._device = { opened: true }
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)

    await d.applyPhoneWorkMode(PhoneWorkMode.Android)

    expect(d._phoneWorkModeRuntime).toBe(PhoneWorkMode.Android)
    expect(d.send).not.toHaveBeenCalled()
  })

  test('applyPhoneWorkMode inner mode switch no-ops when closing or device is not opened', async () => {
    const d = new DongleDriver() as any
    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._lastModeSwitchAt = 0
    d._cfg = { width: 800, height: 480, fps: 60 }
    d._device = { opened: false }
    d._closing = false
    d.send = jest.fn(async () => true)
    d.sleep = jest.fn(async () => undefined)

    await d.applyPhoneWorkMode(PhoneWorkMode.Android)
    expect(d.send).not.toHaveBeenCalled()

    d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
    d._lastModeSwitchAt = 0
    d._device = { opened: true }
    d._closing = true

    await d.applyPhoneWorkMode(PhoneWorkMode.Android)
    expect(d.send).not.toHaveBeenCalled()
  })

  test('setPendingStartupConnectTarget clears target when btMac is nullish', () => {
    const d = new DongleDriver() as any
    d._pendingStartupConnectTarget = {
      btMac: 'AA:BB:CC:DD:EE:FF',
      phoneWorkMode: PhoneWorkMode.CarPlay
    }

    d.setPendingStartupConnectTarget({
      btMac: undefined as any,
      phoneWorkMode: PhoneWorkMode.Android
    })

    expect(d._pendingStartupConnectTarget).toBeNull()
  })

  test('waitForReaderStop returns immediately when reader is already inactive', async () => {
    const d = new DongleDriver() as any
    d._readerActive = false
    d.sleep = jest.fn(async () => undefined)

    await d.waitForReaderStop(50)

    expect(d.sleep).not.toHaveBeenCalled()
  })

  test('emitDongleInfoIfChanged also works when firmware version is undefined', () => {
    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    d._dongleFwVersion = undefined
    d._boxInfo = { productType: 'A15W' }

    d.emitDongleInfoIfChanged()

    expect(emitSpy).toHaveBeenCalledWith('dongle-info', {
      dongleFwVersion: undefined,
      boxInfo: { productType: 'A15W' }
    })
    expect(d._lastDongleInfoEmitKey).toBe('||{"productType":"A15W"}')
  })

  test('handleMessage emits VendorSessionInfo without debug log when DEBUG is false', async () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})
    const { decryptVendorSessionText } = jest.requireMock('@main/helpers/vendorSessionInfo')
    decryptVendorSessionText.mockResolvedValueOnce('decrypted-session')

    const d = new DongleDriver() as any
    const emitSpy = jest.spyOn(d, 'emit')

    const msg = Object.create(VendorSessionInfo.prototype)
    msg.raw = Buffer.from('abcd')

    await d.handleMessage(msg)

    expect(logSpy).not.toHaveBeenCalledWith('[DongleDriver] VendorSessionInfo decrypted-session')
    expect(emitSpy).toHaveBeenCalledWith('message', msg)

    logSpy.mockRestore()
  })

  test('tryResetUnderlyingUsbDevice returns false when dev itself is not an object', async () => {
    const d = new DongleDriver() as any

    await expect(d.tryResetUnderlyingUsbDevice(null)).resolves.toBe(false)
    await expect(d.tryResetUnderlyingUsbDevice('not-an-object')).resolves.toBe(false)
  })

  test('waitForReaderStop uses default timeout when called without argument', async () => {
    const d = new DongleDriver() as any
    d._readerActive = false
    d.sleep = jest.fn(async () => undefined)

    await expect(d.waitForReaderStop()).resolves.toBeUndefined()
    expect(d.sleep).not.toHaveBeenCalled()
  })
})
