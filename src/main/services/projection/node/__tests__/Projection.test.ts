import EventEmitter from 'events'
import { AudioCommand } from '@shared/types/ProjectionEnums'

const requestDevice = jest.fn()

const micInstances: any[] = []
class MockMicrophone extends EventEmitter {
  start = jest.fn()
  stop = jest.fn()
  constructor() {
    super()
    micInstances.push(this)
  }
}

class MockDongleDriver extends EventEmitter {
  static knownDevices = [{ vendorId: 0x1314, productId: 0x1520 }]

  send = jest.fn(async () => true)
  initialise = jest.fn(async () => undefined)
  start = jest.fn(async () => undefined)
  close = jest.fn(async () => undefined)
}

class Plugged {
  constructor(public phoneType: number) {}
}
class Unplugged {}
class VideoData {}
class MediaData {}
class Command {}
class AudioData {
  constructor(public command?: number) {}
}
class SendAudio {
  constructor(public data: Int16Array) {}
}
class SendCommand {
  constructor(public value: string) {}
}
class SendTouch {
  constructor(
    public x: number,
    public y: number,
    public action: number
  ) {}
}

jest.mock('usb', () => ({
  webusb: {
    requestDevice
  }
}))

jest.mock('@main/services/audio', () => ({
  Microphone: MockMicrophone
}))

jest.mock('@main/services/projection/messages', () => ({
  Plugged,
  Unplugged,
  VideoData,
  AudioData,
  MediaData,
  Command,
  SendAudio,
  SendCommand,
  SendTouch
}))

jest.mock('@main/services/projection/driver/DongleDriver', () => ({
  DongleDriver: MockDongleDriver,
  DEFAULT_CONFIG: { phoneConfig: {} }
}))

import Projection from '@main/services/projection/node/Projection'

describe('Projection node wrapper', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    micInstances.length = 0
    requestDevice.mockReset()
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  test('microphone PCM data is forwarded to dongle driver as SendAudio', async () => {
    const p = new Projection({}) as any
    const mic = micInstances[0]

    const chunk = new Int16Array([1, 2, 3])
    mic.emit('data', chunk)

    expect(p.dongleDriver.send).toHaveBeenCalledTimes(1)
    expect(p.dongleDriver.send.mock.calls[0][0]).toBeInstanceOf(SendAudio)
    expect(p.dongleDriver.send.mock.calls[0][0].data).toBe(chunk)
  })

  test('handles Plugged event and emits onmessage plugged', () => {
    const p = new Projection({}) as any
    p.onmessage = jest.fn()

    p.dongleDriver.emit('message', new Plugged(3))

    expect(p.onmessage).toHaveBeenCalledWith({ type: 'plugged' })
  })

  test('audio command start/stop controls microphone', () => {
    const p = new Projection({}) as any
    const mic = micInstances[0]

    p.dongleDriver.emit('message', new AudioData(AudioCommand.AudioSiriStart))
    p.dongleDriver.emit('message', new AudioData(AudioCommand.AudioPhonecallStart))
    p.dongleDriver.emit('message', new AudioData(AudioCommand.AudioSiriStop))
    p.dongleDriver.emit('message', new AudioData(AudioCommand.AudioPhonecallStop))

    expect(mic.start).toHaveBeenCalledTimes(2)
    expect(mic.stop).toHaveBeenCalledTimes(2)
  })

  test('resetDongle opens, resets and closes usb device', async () => {
    const dev = {
      open: jest.fn(async () => undefined),
      reset: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }
    requestDevice.mockResolvedValue(dev)

    const p = new Projection({})
    await p.resetDongle()

    expect(requestDevice).toHaveBeenCalledWith({ filters: MockDongleDriver.knownDevices })
    expect(dev.open).toHaveBeenCalledTimes(1)
    expect(dev.reset).toHaveBeenCalledTimes(1)
    expect(dev.close).toHaveBeenCalledTimes(1)
  })

  test('initialiseAfterReconnect initialises driver and schedules wifiPair', async () => {
    const dev = {
      open: jest.fn(async () => undefined),
      reset: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined)
    }
    requestDevice.mockResolvedValue(dev)

    const p = new Projection({ width: 800, height: 480, fps: 60 }) as any

    await p.initialiseAfterReconnect()

    expect(p.dongleDriver.initialise).toHaveBeenCalledWith(dev)
    expect(p.dongleDriver.start).toHaveBeenCalledTimes(1)

    jest.advanceTimersByTime(15000)
    await Promise.resolve()
    expect(p.dongleDriver.send).toHaveBeenCalled()
  })

  test('sendKey and sendTouch proxy to dongle driver.send', () => {
    const p = new Projection({}) as any

    p.sendKey('frame')
    p.sendTouch({ type: 2, x: 0.5, y: 0.4 })

    expect(p.dongleDriver.send.mock.calls[0][0]).toBeInstanceOf(SendCommand)
    expect(p.dongleDriver.send.mock.calls[1][0]).toBeInstanceOf(SendTouch)
  })

  test('stop closes dongle driver and clears timers', async () => {
    const p = new Projection({}) as any
    p._pairTimeout = setTimeout(() => {}, 1000)
    p._frameInterval = setInterval(() => {}, 1000)

    await p.stop()

    expect(p.dongleDriver.close).toHaveBeenCalledTimes(1)
    expect(p._pairTimeout).toBeNull()
    expect(p._frameInterval).toBeNull()
  })
})
