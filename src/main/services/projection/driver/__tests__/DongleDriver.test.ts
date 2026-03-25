import {
  AndroidWorkMode,
  DongleDriver,
  DriverStateError
} from '@main/services/projection/driver/DongleDriver'
import {
  SendBluetoothPairedList,
  SendCommand,
  SendGnssData,
  SendOpen
} from '@main/services/projection/messages/sendable'
import { PhoneWorkMode } from '@shared/types'
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
})
