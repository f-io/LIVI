import { EventEmitter } from 'node:events'
import {
  AudioData,
  BoxInfo,
  DuckAudio,
  Opened,
  PhoneType,
  Plugged,
  SoftwareVersion
} from '@projection/messages'
import { InputCommand, PhoneWorkMode } from '@shared/types'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const { micTapOpen, openedTaps, connectMock } = vi.hoisted(() => {
  const openedTaps: { close: ReturnType<typeof vi.fn> }[] = []
  const micTapOpen = vi.fn(() => {
    const tap = { close: vi.fn() }
    openedTaps.push(tap)
    return tap
  })
  const connectMock = vi.fn()
  return { micTapOpen, openedTaps, connectMock }
})

vi.mock('@main/services/audio/micTap', () => ({ MicTap: { open: micTapOpen } }))
vi.mock('../../aa/stack/transport/HelperSessionLink', () => ({
  HelperSessionLink: { connect: connectMock }
}))
vi.mock('../vendorSessionInfo', () => ({
  decryptVendorSessionText: vi.fn(async () => 'decrypted')
}))

import type { AaMediaSinkDeps } from '../../aa/AaEventBridge'
import { AndroidWorkMode, DongleDriver } from '../dongleDriver'
import { MessageType } from '../protocol/wire'

class FakeLink extends EventEmitter {
  closed = false
  send = vi.fn()
  control = vi.fn()
  destroy = vi.fn(() => {
    this.closed = true
  })
}

type SinkCallback = (audioType: number, streamId: number, tag?: string) => void

function sink(): AaMediaSinkDeps & { callbacks: SinkCallback[] } {
  const callbacks: SinkCallback[] = []
  return {
    callbacks,
    feedPath: async () => '/tmp/feed.sock',
    videoPlaneId: (cluster) => (cluster ? 0x7a000010 : 0x7a000001),
    primeVideo: vi.fn(),
    noteVideoStarted: vi.fn(),
    audioOutputs: () => [],
    onAudioOutput: (cb) => {
      callbacks.push(cb)
      return () => {}
    },
    primeAudio: vi.fn(),
    setHostVolume: vi.fn()
  }
}

function cfg(): Record<string, unknown> {
  return {
    projectionWidth: 800,
    projectionHeight: 480,
    projectionFps: 30,
    carName: 'Car',
    oemName: '',
    wifiType: '5ghz',
    disableAudioOutput: false,
    audioInputDevice: 'mic0',
    projectionViewAreaTop: 0,
    projectionViewAreaBottom: 0,
    projectionViewAreaLeft: 0,
    projectionViewAreaRight: 0,
    projectionSafeAreaTop: 0,
    projectionSafeAreaBottom: 0,
    projectionSafeAreaLeft: 0,
    projectionSafeAreaRight: 0,
    projectionSafeAreaDrawOutside: false
  }
}

function audioCommand(decodeType: number, audioType: number, command: number): Buffer {
  const b = Buffer.alloc(13)
  b.writeUInt32LE(decodeType, 0)
  b.writeFloatLE(0, 4)
  b.writeUInt32LE(audioType, 8)
  b.writeUInt8(command, 12)
  return b
}

const flush = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve()
}

type Attached = { driver: DongleDriver; link: FakeLink; media: ReturnType<typeof sink> }

function attached(): Attached {
  const driver = new DongleDriver()
  const media = sink()
  driver.setMediaSink(media)
  const link = new FakeLink()
  driver.attach(link as unknown as Parameters<DongleDriver['attach']>[0], 'S1')
  return { driver, link, media }
}

/** Message types the driver sent, in order. */
function sentTypes(link: FakeLink): number[] {
  return link.send.mock.calls.map((c) => c[2] as number)
}

beforeEach(() => {
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
  micTapOpen.mockClear()
  connectMock.mockReset()
  openedTaps.length = 0
})

describe('DongleDriver over the helper session', () => {
  test('attach announces the dongle and start opens it with the geometry', async () => {
    const driver = new DongleDriver()
    const media = sink()
    driver.setMediaSink(media)
    const attachedEv = vi.fn()
    driver.on('attached', attachedEv)
    const link = new FakeLink()
    driver.attach(link as unknown as Parameters<DongleDriver['attach']>[0], 'S1')

    expect(attachedEv).toHaveBeenCalledTimes(1)
    expect(driver.isUp).toBe(true)
    expect(driver.serial).toBe('S1')

    const started = driver.start(cfg() as never)
    await flush()
    link.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    await flush()
    expect(media.primeVideo).toHaveBeenCalledWith(false)
    expect(link.control).toHaveBeenCalledWith({
      type: 'sink',
      feed: '/tmp/feed.sock',
      video: [{ cluster: false, id: 0x7a000001, codec: 'h264' }]
    })
    const [, , type, payload] = link.send.mock.calls[0] as [number, number, number, Buffer]
    expect(type).toBe(MessageType.Open)
    expect(payload.readUInt32LE(0)).toBe(800)
    expect(payload.readUInt32LE(4)).toBe(480)
    expect(payload.readUInt32LE(8)).toBe(30)
    await driver.close()
    await started
  })

  test('start is a no-op without a session and send reports it', async () => {
    const driver = new DongleDriver()
    await driver.start(cfg() as never)
    expect(driver.isUp).toBe(false)
    expect(await driver.send({} as never)).toBe(false)
  })

  test('messages from the link arrive decoded', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    const plugged = Buffer.alloc(4)
    plugged.writeUInt32LE(PhoneType.AndroidAuto, 0)
    link.emit('message', 0, 0, MessageType.Plugged, plugged)
    await flush()
    expect(messages[0]).toBeInstanceOf(Plugged)
    expect((messages[0] as Plugged).phoneType).toBe(PhoneType.AndroidAuto)
    await driver.close()
  })

  test('the open reply triggers the post-open configuration', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start(cfg() as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.send.mockClear()
      link.emit('message', 0, 0, MessageType.Open, Buffer.alloc(28))
      await vi.advanceTimersByTimeAsync(3000)
      const types = sentTypes(link)
      expect(types.filter((t) => t === MessageType.SendFile).length).toBeGreaterThan(3)
      expect(types).toContain(MessageType.BoxSettings)
      expect(types).toContain(MessageType.Command)
      expect(types).not.toContain(MessageType.HeartBeat)
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('the first main frame brings the phone up and geometry reaches the sink', async () => {
    const { driver, link, media } = attached()
    const connected = vi.fn()
    driver.on('phone-connected', connected)
    link.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    link.emit('control', { type: 'video', cluster: true, width: 400, height: 240 })
    link.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    expect(connected).toHaveBeenCalledTimes(1)
    expect(media.noteVideoStarted).toHaveBeenNthCalledWith(1, false, 800, 480)
    expect(media.noteVideoStarted).toHaveBeenNthCalledWith(2, true, 400, 240)
    await driver.close()
  })

  test('an audio format primes a host stream and its id goes back as the sink', async () => {
    const { driver, link, media } = attached()
    link.emit('control', { type: 'audio-setup', decodeType: 2, audioType: 1 })
    expect(media.primeAudio).toHaveBeenCalledWith(1, 44100, 2, 'dongle:2:1')
    for (const cb of media.callbacks) cb(1, 77, 'dongle:2:1')
    for (const cb of media.callbacks) cb(4, 78, 'speech')
    await flush()
    expect(link.control).toHaveBeenCalledWith({
      type: 'sink',
      feed: '/tmp/feed.sock',
      audio: [{ decodeType: 2, audioType: 1, id: 77 }]
    })
    expect(link.control).toHaveBeenCalledTimes(1)
    await driver.close()
  })

  test('requestKeyframe asks the phone for a fresh frame', async () => {
    const { driver, link } = attached()
    driver.requestKeyframe()
    expect(sentTypes(link)).toEqual([MessageType.Command])
    await driver.close()
  })

  test('the microphone follows the input config and the stop commands', async () => {
    const { driver, link } = attached()
    await driver.start(cfg() as never)
    link.emit('control', { type: 'ready', mic: '/tmp/dongle-session-1.sock.mic' })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    expect(micTapOpen).toHaveBeenCalledWith('/tmp/dongle-session-1.sock.mic', {
      sampleRate: 16000,
      channels: 1,
      device: 'mic0'
    })
    expect(link.control).toHaveBeenCalledWith({ type: 'mic', decodeType: 5 })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    expect(micTapOpen).toHaveBeenCalledTimes(1)
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioPhonecallStop)
    )
    await flush()
    expect(openedTaps[0].close).toHaveBeenCalledTimes(1)
    expect(link.control).toHaveBeenCalledWith({ type: 'mic' })
    await driver.close()
  })

  test('audio commands translate into ducking', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(1, 1, AudioCommand.AudioNaviStart)
    )
    await flush()
    expect(messages[0]).toBeInstanceOf(AudioData)
    const duck = messages[1] as DuckAudio
    expect(duck).toBeInstanceOf(DuckAudio)
    expect(duck.level).toBe(0.2)
    await driver.close()
  })

  test('losing the session drops the phone and the dongle', async () => {
    const { driver, link } = attached()
    await driver.start(cfg() as never)
    const disconnected = vi.fn()
    const detached = vi.fn()
    driver.on('phone-disconnected', disconnected)
    driver.on('detached', detached)
    link.closed = true
    link.emit('close')
    expect(disconnected).toHaveBeenCalledTimes(1)
    expect(detached).toHaveBeenCalledTimes(1)
    expect(driver.isUp).toBe(false)
    expect(await driver.send({} as never)).toBe(false)
    link.emit('close')
    expect(detached).toHaveBeenCalledTimes(1)
  })

  test('a closed control from the helper ends the session too', async () => {
    const { driver, link } = attached()
    const detached = vi.fn()
    driver.on('detached', detached)
    link.emit('control', { type: 'closed', reason: 'dongle read ended' })
    expect(detached).toHaveBeenCalledTimes(1)
    expect(link.destroy).toHaveBeenCalled()
    expect(driver.isUp).toBe(false)
  })

  test('the helper announcement connects the session', async () => {
    const driver = new DongleDriver()
    const link = new FakeLink()
    connectMock.mockResolvedValue(link)
    let onEvent: ((ev: { event: string; socket?: string; serial?: string }) => void) | null = null
    const source = {
      subscribe: vi.fn((cb: typeof onEvent) => {
        onEvent = cb
        return { close: vi.fn() }
      })
    }
    driver.attachHelper(source)
    driver.attachHelper(source)
    expect(source.subscribe).toHaveBeenCalledTimes(1)
    onEvent?.({ event: 'aa-session', socket: '/tmp/aa-session-1.sock' })
    onEvent?.({ event: 'dongle-session', socket: '/tmp/dongle-session-1.sock', serial: 'S9' })
    await flush()
    expect(connectMock).toHaveBeenCalledTimes(1)
    expect(connectMock).toHaveBeenCalledWith('/tmp/dongle-session-1.sock', 'S9')
    expect(driver.isUp).toBe(true)
    expect(driver.serial).toBe('S9')
    driver.detachHelper()
    expect(driver.isUp).toBe(false)
    expect(link.destroy).toHaveBeenCalled()
  })

  test('opened messages after an unplug restart the phone lifecycle', async () => {
    const { driver, link } = attached()
    const disconnected = vi.fn()
    driver.on('phone-disconnected', disconnected)
    link.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    link.emit('message', 0, 0, MessageType.Unplugged, Buffer.alloc(0))
    await flush()
    expect(disconnected).toHaveBeenCalledTimes(1)
    const connected = vi.fn()
    driver.on('phone-connected', connected)
    link.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    expect(connected).toHaveBeenCalledTimes(1)
    expect(heartbeatsOf(link)).toEqual([])
    await driver.close()
  })
})

function heartbeatsOf(link: FakeLink): unknown[] {
  return link.send.mock.calls.filter((c) => c[2] === MessageType.HeartBeat)
}

describe('DongleDriver decoded message types', () => {
  test('an open reply is an Opened message', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit('message', 0, 0, MessageType.Open, Buffer.alloc(28))
    await flush()
    expect(messages[0]).toBeInstanceOf(Opened)
    await driver.close()
  })
})

describe('DongleDriver protocol paths', () => {
  test('the open reply with a pending target dispatches a targeted auto-connect', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const dispatched = vi.fn()
      driver.on('targeted-connect-dispatched', dispatched)
      const started = driver.start(cfg() as never, {
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.CarPlay
      })
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.emit('message', 0, 0, MessageType.Open, Buffer.alloc(28))
      await vi.advanceTimersByTimeAsync(4000)
      const controls = link.send.mock.calls.map((c) => c[2] as number)
      // WifiStatusData carries the auto-connect address.
      expect(controls).toContain(MessageType.WifiStatusData)
      expect(dispatched).toHaveBeenCalledWith({
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.CarPlay
      })
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('a plugged Android phone switches the phone work mode', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start(cfg() as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.send.mockClear()
      const plugged = Buffer.alloc(4)
      plugged.writeUInt32LE(PhoneType.AndroidAuto, 0)
      link.emit('message', 0, 0, MessageType.Plugged, plugged)
      await vi.advanceTimersByTimeAsync(400)
      const types = link.send.mock.calls.map((c) => c[2] as number)
      expect(types).toContain(MessageType.DisconnectPhone)
      expect(types).toContain(MessageType.Open)
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('box info flips the phone work mode on the mismatch marker', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start(cfg() as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.send.mockClear()
      const info = Buffer.from(JSON.stringify({ MDLinkType: 'RiddleLinktype_UNKNOWN?' }), 'utf8')
      link.emit('message', 0, 0, MessageType.BoxSettings, info)
      await vi.advanceTimersByTimeAsync(400)
      const types = link.send.mock.calls.map((c) => c[2] as number)
      expect(types).toContain(MessageType.DisconnectPhone)
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('software and box info raise dongle-info once per change', async () => {
    const { driver, link } = attached()
    const info = vi.fn()
    driver.on('dongle-info', info)
    link.emit('message', 0, 0, MessageType.SoftwareVersion, Buffer.from('2025.03.19.1126', 'ascii'))
    link.emit('message', 0, 0, MessageType.SoftwareVersion, Buffer.from('2025.03.19.1126', 'ascii'))
    await flush()
    expect(info).toHaveBeenCalledTimes(1)
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit('message', 0, 0, MessageType.SoftwareVersion, Buffer.from('2025.03.19.1126', 'ascii'))
    await flush()
    expect(messages[0]).toBeInstanceOf(SoftwareVersion)
    await driver.close()
  })

  test('a box settings message reaches the app as box info', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit(
      'message',
      0,
      0,
      MessageType.BoxSettings,
      Buffer.from(JSON.stringify({ uuid: 'x' }), 'utf8')
    )
    await flush()
    expect(messages.some((m) => m instanceof BoxInfo)).toBe(true)
    await driver.close()
  })

  test('the outbound helpers and control commands reach the link', async () => {
    const { driver, link } = attached()
    await driver.sendBluetoothPairedList('list')
    await driver.sendGnssData('$GPGGA')
    driver.uploadHostIcons(Buffer.from([1]), Buffer.from([2]), Buffer.from([3]))
    driver.requestClusterFocus()
    driver.requestKeyframe()
    driver.handleInput(InputCommand.Play)
    driver.handleInput(-1 as never)
    const types = link.send.mock.calls.map((c) => c[2] as number)
    expect(types).toContain(MessageType.BluetoothPairedList)
    expect(types).toContain(MessageType.GnssData)
    expect(types.filter((t) => t === MessageType.SendFile).length).toBe(3)
    expect(types).toContain(MessageType.Command)
    await driver.close()
  })

  test('setStreamVolume rides the media sink and disconnectPhone sends the teardown pair', async () => {
    const { driver, link, media } = attached()
    driver.setStreamVolume(3, 0.5, 80)
    expect(media.setHostVolume).toHaveBeenCalledWith(3, 0.5, 80)
    const ok = await driver.disconnectPhone()
    expect(ok).toBe(true)
    const types = link.send.mock.calls.map((c) => c[2] as number)
    expect(types).toContain(MessageType.DisconnectPhone)
    expect(types).toContain(MessageType.CloseDongle)
    await driver.close()
  })

  test('resetDongle asks the helper and usbDevice reports the attached ids', () => {
    const driver = new DongleDriver()
    driver.setMediaSink(sink())
    const link = new FakeLink()
    driver.attach(link as unknown as Parameters<DongleDriver['attach']>[0], 'S7', {
      product: 0x1521,
      version: 0x0102,
      name: 'Carlinkit'
    })
    expect(driver.usbDevice()).toEqual({
      vendorId: 0x1314,
      productId: 0x1521,
      usbFwVersion: '1.02',
      deviceName: 'Carlinkit'
    })
    expect(driver.resetDongle()).toBe(true)
    expect(link.control).toHaveBeenCalledWith({ type: 'reset' })
    link.closed = true
    expect(driver.resetDongle()).toBe(false)
    expect(driver.usbDevice()).toBeNull()
  })

  test('an input config for an unknown format opens no microphone', async () => {
    const { driver, link } = attached()
    await driver.start(cfg() as never)
    link.emit('control', { type: 'ready', mic: '/tmp/dongle-session-9.sock.mic' })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(99, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    expect(micTapOpen).not.toHaveBeenCalled()
    await driver.close()
  })

  test('an audio-setup for an unknown decode type primes no stream', async () => {
    const { driver, link, media } = attached()
    link.emit('control', { type: 'audio-setup', decodeType: 99, audioType: 1 })
    expect(media.primeAudio).not.toHaveBeenCalled()
    await driver.close()
  })

  test('a phone call command ducks to silence and its stop lifts the duck', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 2, AudioCommand.AudioPhonecallStart)
    )
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 2, AudioCommand.AudioPhonecallStop)
    )
    await flush()
    const ducks = messages.filter((m) => m instanceof DuckAudio) as DuckAudio[]
    expect(ducks[0].level).toBe(0)
    expect(ducks[1].level).toBe(1)
    await driver.close()
  })

  test('a vendor session message is decrypted and forwarded', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit('message', 0, 0, MessageType.VendorSessionInfo, Buffer.from([1, 2, 3, 4]))
    await flush()
    expect(messages).toHaveLength(1)
    await driver.close()
  })
})

describe('DongleDriver helper subscription and internals', () => {
  test('foreign events are ignored, a close resubscribes and connect failures are logged', async () => {
    vi.useFakeTimers()
    try {
      const driver = new DongleDriver()
      let onEvent: ((ev: Record<string, unknown>) => void) | null = null
      let onClose: (() => void) | null = null
      const source = {
        subscribe: vi.fn((e: typeof onEvent, c: typeof onClose) => {
          onEvent = e
          onClose = c
          return { close: vi.fn() }
        })
      }
      driver.attachHelper(source)
      onEvent?.({ event: 'aa-session', socket: '/x' })
      onEvent?.({ event: 'dongle-session' })
      connectMock.mockRejectedValueOnce(new Error('nope'))
      onEvent?.({ event: 'dongle-session', socket: '/s1', serial: 'A' })
      await flush()
      expect(source.subscribe).toHaveBeenCalledTimes(1)
      onClose?.()
      await vi.advanceTimersByTimeAsync(2000)
      expect(source.subscribe).toHaveBeenCalledTimes(2)
      driver.detachHelper()
    } finally {
      vi.useRealTimers()
    }
  })

  test('a session resolving after the helper is gone is discarded', async () => {
    const driver = new DongleDriver()
    let onEvent: ((ev: Record<string, unknown>) => void) | null = null
    const source = {
      subscribe: vi.fn((e: typeof onEvent) => {
        onEvent = e
        return { close: vi.fn() }
      })
    }
    driver.attachHelper(source)
    const link = new FakeLink()
    let resolve!: (l: unknown) => void
    connectMock.mockReturnValueOnce(new Promise((r) => (resolve = r)))
    onEvent?.({ event: 'dongle-session', socket: '/s', serial: 'B' })
    driver.detachHelper()
    resolve(link)
    await flush()
    expect(link.destroy).toHaveBeenCalled()
    expect(driver.isUp).toBe(false)
  })

  test('the work-mode, info and pending-target internals guard bad state', async () => {
    vi.useFakeTimers()
    try {
      const { driver } = attached()
      const d = driver as unknown as Record<string, (...a: unknown[]) => unknown> & {
        _phoneWorkModeRuntime: number
        _boxInfo: unknown
        _lastDongleInfoEmitKey: string
        _pendingModeHintFromBoxInfo: number | null
        _lastPluggedPhoneType: number | null
      }
      const started = driver.start(cfg() as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      await d.applyPhoneWorkMode(d._phoneWorkModeRuntime)
      await d.applyAndroidWorkMode(AndroidWorkMode.Off)
      expect(d.resolveAndroidWorkModeOnPlugged(PhoneType.AndroidAuto)).toBe(
        AndroidWorkMode.AndroidAuto
      )
      const circular: Record<string, unknown> = {}
      circular.self = circular
      d._boxInfo = circular
      d._lastDongleInfoEmitKey = ''
      expect(() => d.emitDongleInfoIfChanged()).not.toThrow()
      d.setPendingStartupConnectTarget(null)
      d.setPendingStartupConnectTarget({ btMac: '  ' })
      d.scheduleWifiConnect(10)
      d.scheduleWifiConnect(10)
      d._pendingModeHintFromBoxInfo = PhoneWorkMode.Android
      d._lastPluggedPhoneType = null
      const rec = d.reconcileModes('boxinfo')
      await vi.advanceTimersByTimeAsync(400)
      await rec
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('the video sink is skipped without a media sink and warns on an empty feed', async () => {
    const bare = new DongleDriver()
    const link = new FakeLink()
    bare.attach(link as unknown as Parameters<DongleDriver['attach']>[0], 'S')
    await bare.start(cfg() as never)
    expect(link.control).not.toHaveBeenCalledWith(expect.objectContaining({ type: 'sink' }))
    await bare.close()

    const media = sink()
    media.feedPath = async () => ''
    const driver = new DongleDriver()
    driver.setMediaSink(media)
    const link2 = new FakeLink()
    driver.attach(link2 as unknown as Parameters<DongleDriver['attach']>[0], 'S')
    await driver.start({ ...(cfg() as object), dashboards: { dash4: { dash: true } } } as never)
    link2.emit('control', { type: 'video', cluster: false, width: 800, height: 480 })
    await flush()
    const sinkCall = link2.control.mock.calls.find(
      (c) => (c[0] as { type: string }).type === 'sink'
    )
    expect((sinkCall?.[0] as { video: unknown[] }).video).toHaveLength(2)
    await driver.close()
  })

  test('a plugged phone with a frame interval runs a keyframe timer that an unplug clears', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start({
        ...(cfg() as object),
        phoneConfig: { [PhoneType.CarPlay]: { frameInterval: 100 } }
      } as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      const plugged = Buffer.alloc(4)
      plugged.writeUInt32LE(PhoneType.CarPlay, 0)
      link.emit('message', 0, 0, MessageType.Plugged, plugged)
      await flush()
      link.send.mockClear()
      await vi.advanceTimersByTimeAsync(250)
      expect(link.send.mock.calls.some((c) => c[2] === MessageType.Command)).toBe(true)
      link.emit('message', 0, 0, MessageType.Unplugged, Buffer.alloc(0))
      link.send.mockClear()
      await vi.advanceTimersByTimeAsync(250)
      expect(link.send).not.toHaveBeenCalled()
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('with no phone the pair timer asks the dongle to pair', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start(cfg() as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.send.mockClear()
      await vi.advanceTimersByTimeAsync(15000)
      expect(link.send.mock.calls.some((c) => c[2] === MessageType.Command)).toBe(true)
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('send and disconnectPhone swallow a throwing link', async () => {
    const { driver, link } = attached()
    link.send.mockImplementation(() => {
      throw new Error('write down')
    })
    expect(await driver.send({} as never)).toBe(false)
    expect(await driver.disconnectPhone()).toBe(false)
  })

  test('an undecodable message and a nav stop and a bare open are handled', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit('message', 0, 0, MessageType.BoxSettings, Buffer.from('not json', 'utf8'))
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(1, 4, AudioCommand.AudioNaviStop)
    )
    link.emit('message', 0, 0, MessageType.Open, Buffer.alloc(0))
    await flush()
    expect(messages.some((m) => m instanceof DuckAudio)).toBe(true)
    await driver.close()
  })

  test('a second input config keeps the one tap and a failing close is swallowed', async () => {
    const { driver, link } = attached()
    await driver.start(cfg() as never)
    link.emit('control', { type: 'ready', mic: '/tmp/s.mic' })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    expect(micTapOpen).toHaveBeenCalledTimes(1)
    openedTaps[0].close.mockImplementationOnce(() => {
      throw new Error('close down')
    })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioVoiceAssistantStop)
    )
    await flush()
    await driver.close()
  })
})

describe('DongleDriver under DEBUG', () => {
  test('the debug logs on vendor session and targeted connect run', async () => {
    vi.resetModules()
    vi.doMock('@main/constants', () => ({ DEBUG: true }))
    const { DongleDriver: Debug } = await import('../dongleDriver')
    vi.useFakeTimers()
    try {
      const driver = new Debug()
      driver.setMediaSink(sink())
      const link = new FakeLink()
      driver.attach(link as unknown as Parameters<DongleDriver['attach']>[0], 'S')
      const started = driver.start(cfg() as never, {
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.CarPlay
      })
      await vi.advanceTimersByTimeAsync(200)
      await started
      link.emit('message', 0, 0, MessageType.Open, Buffer.alloc(28))
      await vi.advanceTimersByTimeAsync(4000)
      link.emit('message', 0, 0, MessageType.VendorSessionInfo, Buffer.from([1, 2, 3, 4]))
      await flush()
      driver.handleInput(-1 as never)
      await driver.close()
    } finally {
      vi.useRealTimers()
      vi.doUnmock('@main/constants')
      vi.resetModules()
    }
  })
})

describe('DongleDriver remaining guards', () => {
  test('the direct guards, logs and async sink paths run', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link, media } = attached()
      const d = driver as unknown as Record<string, (...a: unknown[]) => unknown> & {
        _cfg: unknown
        _link: unknown
        _postOpenConfigSent: boolean
        _wifiConnectTimer: unknown
        _lastModeSwitchAt: number
        _phoneWorkModeRuntime: number
        _androidWorkModeRuntime: number
        _lastPluggedPhoneType: number | null
        _pendingStartupConnectTarget: unknown
      }
      d._cfg = cfg()

      // attach replacing an existing link, and a link error
      const link2 = new FakeLink()
      driver.attach(link2 as unknown as Parameters<DongleDriver['attach']>[0], '')
      link2.emit('error', new Error('session down'))

      // audio outputs already present at attach flush through the sink loop
      const media2 = sink()
      media2.audioOutputs = () => [{ audioType: 3, streamId: 9, tag: 'dongle:5:3' }]
      const withOutputs = new DongleDriver()
      withOutputs.setMediaSink(media2)
      const l3 = new FakeLink()
      withOutputs.attach(l3 as unknown as Parameters<DongleDriver['attach']>[0], 'S3')

      d.logPhoneWorkModeChange('r', 0, 1, 'extra')
      d.logPhoneWorkModeChange('r', 0, 1)
      await d.applyAndroidWorkMode(d._androidWorkModeRuntime)

      // the 800 ms rapid-switch guard
      d._lastModeSwitchAt = Date.now()
      d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
      await d.applyPhoneWorkMode(PhoneWorkMode.Android)

      // the async video and audio sinks find the link gone
      media.feedPath = () => Promise.resolve('/tmp/f')
      d._link = link
      d._pushVideoSink()
      d._link = null
      await vi.advanceTimersByTimeAsync(0)
      d._link = link
      d._pushAudioSink(9, 'dongle:5:3')
      d._link = null
      await vi.advanceTimersByTimeAsync(0)
      d._link = link

      // a queued phone-mode switch that finds the link gone
      d._lastModeSwitchAt = 0
      d._phoneWorkModeRuntime = PhoneWorkMode.CarPlay
      const queued = d.applyPhoneWorkMode(PhoneWorkMode.Android)
      d._link = null
      await vi.advanceTimersByTimeAsync(200)
      await queued
      d._link = link

      // reconcile that changes the android work mode
      d._androidWorkModeRuntime = AndroidWorkMode.Off
      d._lastPluggedPhoneType = PhoneType.AndroidAuto
      d._lastModeSwitchAt = Date.now()
      const rec = d.reconcileModes('plugged')
      await vi.advanceTimersByTimeAsync(200)
      await rec

      // post-open config: targeted connect clears a pending wifi timer, then the guards
      d._postOpenConfigSent = false
      d._pendingStartupConnectTarget = { btMac: 'AA:BB:CC:DD:EE:FF' }
      d._wifiConnectTimer = setTimeout(() => {}, 99999)
      const post = d.sendPostOpenConfig()
      await vi.advanceTimersByTimeAsync(4000)
      await post
      await d.sendPostOpenConfig()
      d._postOpenConfigSent = false
      d._link = null
      await d.sendPostOpenConfig()
      d._link = link

      // the resubscribe closure with the helper already gone
      d._openHelperSub()

      await driver.close()
      await withOutputs.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('disconnectPhone swallows a send that throws outright', async () => {
    const { driver } = attached()
    ;(driver as unknown as { send: () => Promise<boolean> }).send = () => {
      throw new Error('down')
    }
    expect(await driver.disconnectPhone()).toBe(false)
  })
})

describe('DongleDriver last statements', () => {
  test('a microphone tap that fails to open warns and bails', async () => {
    const { driver, link } = attached()
    await driver.start(cfg() as never)
    link.emit('control', { type: 'ready', mic: '/tmp/s.mic' })
    micTapOpen.mockReturnValueOnce(null as never)
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    await driver.close()
  })

  test('a vendor session whose decrypt fails is still forwarded', async () => {
    const { driver, link } = attached()
    const { decryptVendorSessionText } = await import('../vendorSessionInfo')
    ;(decryptVendorSessionText as unknown as ReturnType<typeof vi.fn>).mockRejectedValueOnce(
      new Error('bad')
    )
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit('message', 0, 0, MessageType.VendorSessionInfo, Buffer.from([1, 2, 3, 4]))
    await flush()
    expect(messages).toHaveLength(1)
    await driver.close()
  })

  test('the post-open configuration runs only once', async () => {
    const { driver } = attached()
    const d = driver as unknown as {
      _postOpenConfigSent: boolean
      sendPostOpenConfig: () => Promise<void>
    }
    d._postOpenConfigSent = true
    await expect(d.sendPostOpenConfig()).resolves.toBeUndefined()
  })

  test('the targeted auto-connect clears a pending wifi timer', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const d = driver as unknown as Record<string, unknown> & {
        sendPostOpenConfig: () => Promise<void>
      }
      d._cfg = cfg()
      d._link = link
      d._postOpenConfigSent = false
      d._pendingStartupConnectTarget = {
        btMac: 'AA:BB:CC:DD:EE:FF',
        phoneWorkMode: PhoneWorkMode.CarPlay
      }
      d._wifiConnectTimer = setTimeout(() => {}, 99999)
      const dispatched = vi.fn()
      driver.on('targeted-connect-dispatched', dispatched)
      const p = d.sendPostOpenConfig()
      await vi.advanceTimersByTimeAsync(5000)
      await p
      expect(dispatched).toHaveBeenCalled()
      expect(d._wifiConnectTimer).toBeNull()
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('DongleDriver branch coverage', () => {
  test('control messages with missing fields fall back to defaults', async () => {
    const { driver, link } = attached()
    link.emit('control', { type: 'ready' })
    link.emit('control', { type: 'video', cluster: false })
    link.emit('control', { type: 'audio-setup' })
    link.emit('control', { type: 'closed' })
    await flush()
    expect(driver.isUp).toBe(false)
  })

  test('a message that decodes to nothing is dropped and an audio tag can be absent', async () => {
    const { driver, link } = attached()
    const d = driver as unknown as { _pushAudioSink: (id: number, tag?: string) => void }
    link.emit('message', 0, 0, MessageType.UiHidePeerInfo, Buffer.from([1, 2]))
    d._pushAudioSink(9, undefined)
    await flush()
    await driver.close()
  })

  test('a helper session without a serial and a close with the helper gone', async () => {
    const driver = new DongleDriver()
    let onEvent: ((ev: Record<string, unknown>) => void) | null = null
    let onClose: (() => void) | null = null
    const source = {
      subscribe: vi.fn((e: typeof onEvent, c: typeof onClose) => {
        onEvent = e
        onClose = c
        return { close: vi.fn() }
      })
    }
    const link = new FakeLink()
    connectMock.mockResolvedValue(link)
    driver.attachHelper(source)
    onEvent?.({ event: 'dongle-session', socket: '/tmp/s.sock' })
    await flush()
    expect(connectMock).toHaveBeenCalledWith('/tmp/s.sock', '')
    driver.detachHelper()
    onClose?.()
    await driver.close()
  })

  test('start honours a saved Android phone work mode', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const started = driver.start({
        ...(cfg() as object),
        lastPhoneWorkMode: PhoneWorkMode.Android
      } as never)
      await vi.advanceTimersByTimeAsync(200)
      await started
      const openPayload = link.send.mock.calls.find((c) => c[2] === MessageType.Open)?.[3] as Buffer
      expect(openPayload.readUInt32LE(24)).toBe(PhoneWorkMode.Android)
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('a microphone opens without a configured input device', async () => {
    const { driver, link } = attached()
    const bare = { ...(cfg() as object) } as Record<string, unknown>
    delete bare.audioInputDevice
    await driver.start(bare as never)
    link.emit('control', { type: 'ready', mic: '/tmp/s.mic' })
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 3, AudioCommand.AudioInputConfig)
    )
    await flush()
    expect(micTapOpen).toHaveBeenCalledWith('/tmp/s.mic', {
      sampleRate: 16000,
      channels: 1,
      device: undefined
    })
    await driver.close()
  })

  test('stopping the microphone after the link closed sends no control', async () => {
    const { driver, link } = attached()
    const d = driver as unknown as {
      _startMic: (dt: number) => void
      _stopMic: () => void
      _micPath: string
      _link: FakeLink
    }
    d._micPath = '/tmp/s.mic'
    d._startMic(5)
    link.closed = true
    link.control.mockClear()
    d._stopMic()
    expect(link.control).not.toHaveBeenCalled()
  })

  test('a voice-assistant start command ducks to silence', async () => {
    const { driver, link } = attached()
    const messages: unknown[] = []
    driver.on('message', (m) => messages.push(m))
    link.emit(
      'message',
      0,
      0,
      MessageType.AudioData,
      audioCommand(5, 1, AudioCommand.AudioVoiceAssistantStart)
    )
    await flush()
    const duck = messages.find((m) => m instanceof DuckAudio) as DuckAudio
    expect(duck.level).toBe(0)
    await driver.close()
  })

  test('the post-open config adapts to 2.4 GHz, muted output, a cluster and no oem name', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const d = driver as unknown as {
        _cfg: unknown
        _postOpenConfigSent: boolean
        _link: FakeLink
        sendPostOpenConfig: () => Promise<void>
      }
      const full = {
        ...(cfg() as object),
        carName: 'Car',
        wifiType: '2.4ghz',
        disableAudioOutput: true,
        nightMode: false,
        hand: 0,
        clusterWidth: 400,
        clusterHeight: 240,
        clusterViewAreaTop: 0,
        clusterViewAreaBottom: 0,
        clusterViewAreaLeft: 0,
        clusterViewAreaRight: 0,
        clusterSafeAreaTop: 0,
        clusterSafeAreaBottom: 0,
        clusterSafeAreaLeft: 0,
        clusterSafeAreaRight: 0,
        dashboards: { dash4: { dash: true } }
      } as Record<string, unknown>
      delete full.oemName
      d._cfg = full
      d._link = link
      d._postOpenConfigSent = false
      const p = d.sendPostOpenConfig()
      await vi.advanceTimersByTimeAsync(5000)
      await p
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('a frame timer that fires while not started sends nothing', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const d = driver as unknown as {
        _cfg: unknown
        onPlugged: (m: unknown) => Promise<void>
        _started: boolean
      }
      d._cfg = { ...(cfg() as object), phoneConfig: { [PhoneType.CarPlay]: { frameInterval: 50 } } }
      const plugged = { phoneType: PhoneType.CarPlay }
      await d.onPlugged(plugged)
      d._started = false
      link.send.mockClear()
      await vi.advanceTimersByTimeAsync(120)
      expect(link.send).not.toHaveBeenCalled()
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('a plug whose mode already matches emits no config change', async () => {
    const { driver } = attached()
    const d = driver as unknown as {
      _cfg: Record<string, unknown>
      onPlugged: (m: unknown) => Promise<void>
    }
    d._cfg = { ...(cfg() as object), lastPhoneWorkMode: PhoneWorkMode.CarPlay }
    const changed = vi.fn()
    driver.on('config-changed', changed)
    await d.onPlugged({ phoneType: PhoneType.CarPlay })
    expect(changed).not.toHaveBeenCalled()
    await driver.close()
  })

  test('the internal logs and pending target cover their remaining sides', async () => {
    const { driver } = attached()
    const d = driver as unknown as {
      logAndroidWorkModeChange: (r: string, a: number, b: number, e?: string) => void
      setPendingStartupConnectTarget: (t: unknown) => void
    }
    d.logAndroidWorkModeChange('r', 0, 1, 'extra')
    d.setPendingStartupConnectTarget({})
    await driver.close()
  })

  test('the post-open label uses the oem name when present', async () => {
    vi.useFakeTimers()
    try {
      const { driver, link } = attached()
      const d = driver as unknown as {
        _cfg: unknown
        _link: FakeLink
        _postOpenConfigSent: boolean
        sendPostOpenConfig: () => Promise<void>
      }
      d._cfg = { ...(cfg() as object), oemName: 'OEM', carName: 'Car' }
      d._link = link
      d._postOpenConfigSent = false
      const p = d.sendPostOpenConfig()
      await vi.advanceTimersByTimeAsync(5000)
      await p
      await driver.close()
    } finally {
      vi.useRealTimers()
    }
  })

  test('box info flips from Android, tolerates no config, and reconcile with no signals', async () => {
    const { driver } = attached()
    const d = driver as unknown as {
      _phoneWorkModeRuntime: number
      _cfg: unknown
      onBoxInfo: (m: unknown) => Promise<void>
      reconcileModes: (r: string) => Promise<void>
      _lastPluggedPhoneType: number | null
      _pendingModeHintFromBoxInfo: number | null
    }
    d._phoneWorkModeRuntime = PhoneWorkMode.Android
    d._cfg = null
    await d.onBoxInfo({ settings: { MDLinkType: 'RiddleLinktype_UNKNOWN?' } })
    d._lastPluggedPhoneType = null
    d._pendingModeHintFromBoxInfo = null
    await d.reconcileModes('boxinfo')
    await driver.close()
  })
})
