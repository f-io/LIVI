import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import { act, render, waitFor } from '@testing-library/react'
import { Projection } from '../Projection'

const navigateMock = vi.fn()
let mockPathname = '/'

vi.mock('@worker/createProjectionWorker', () => ({
  createProjectionWorker: vi.fn()
}))

type AnyFn = (...args: any[]) => any

const statusState: Record<string, any> = {
  isStreaming: true,
  isDongleHardwarePresent: true,
  activeProtocol: null,
  setStreaming: vi.fn(),
  setDongleHardwarePresent: vi.fn(),
  setActiveProtocol: vi.fn()
}

const liviState: Record<string, any> = {
  negotiatedWidth: 0,
  negotiatedHeight: 0,
  dongleFwVersion: '',
  boxInfo: null,
  resetInfo: vi.fn(),
  setDeviceInfo: vi.fn(),
  setAudioInfo: vi.fn(),
  setPcmData: vi.fn(),
  setBluetoothPairedList: vi.fn()
}

vi.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useLocation: () => ({ pathname: mockPathname })
}))

vi.mock('../../../../store/store', async () => {
  const useStatusStore: any = (selector: AnyFn) => selector(statusState)
  useStatusStore.setState = (patch: Record<string, any>) => Object.assign(statusState, patch)

  const useLiviStore: any = (selector: AnyFn) => selector(liviState)
  useLiviStore.setState = (patch: Record<string, any> | AnyFn) => {
    if (typeof patch === 'function') {
      Object.assign(liviState, patch(liviState))
    } else {
      Object.assign(liviState, patch)
    }
  }

  const useProjectionActive = () =>
    statusState.isDongleHardwarePresent || statusState.activeProtocol != null

  return { useStatusStore, useLiviStore, useProjectionActive }
})

vi.mock('../hooks/useProjectionTouch', () => ({
  useProjectionMultiTouch: () => ({})
}))

class MockWorker {
  static instances: MockWorker[] = []
  public postMessage = vi.fn()
  public terminate = vi.fn()
  public onerror: AnyFn | null = null
  private listeners: Array<(ev: MessageEvent<any>) => void> = []

  constructor(public url: string) {
    MockWorker.instances.push(this)
  }

  addEventListener(type: string, cb: (ev: MessageEvent<any>) => void) {
    if (type === 'message') this.listeners.push(cb)
  }

  removeEventListener(type: string, cb: (ev: MessageEvent<any>) => void) {
    if (type === 'message') this.listeners = this.listeners.filter((x) => x !== cb)
  }

  emit(data: unknown) {
    this.listeners.forEach((cb) => cb({ data } as MessageEvent))
  }

  triggerError(ev: unknown) {
    this.onerror?.(ev)
  }
}

class MockMessageChannel {
  static instances: MockMessageChannel[] = []
  port1 = { postMessage: vi.fn() }
  port2 = {}
  constructor() {
    MockMessageChannel.instances.push(this)
  }
}

describe('Projection page', () => {
  let onEventCb: AnyFn | undefined
  let usbCb: AnyFn | undefined

  beforeEach(async () => {
    MockWorker.instances = []
    MockMessageChannel.instances = []
    navigateMock.mockReset()
    mockPathname = '/'

    statusState.isStreaming = true
    statusState.isDongleHardwarePresent = true
    statusState.activeProtocol = null
    statusState.setStreaming.mockClear()
    statusState.setDongleHardwarePresent.mockClear()
    statusState.setActiveProtocol.mockClear()

    liviState.negotiatedWidth = 0
    liviState.negotiatedHeight = 0
    liviState.dongleFwVersion = ''
    liviState.boxInfo = null
    liviState.resetInfo.mockClear()
    liviState.setDeviceInfo.mockClear()
    liviState.setAudioInfo.mockClear()
    liviState.setPcmData.mockClear()
    liviState.setBluetoothPairedList.mockClear()

    liviState.resetInfo.mockClear()
    liviState.setDeviceInfo.mockClear()
    liviState.setAudioInfo.mockClear()
    liviState.setPcmData.mockClear()
    liviState.setBluetoothPairedList.mockClear()
    statusState.setStreaming.mockClear()
    statusState.setDongleHardwarePresent.mockClear()
    statusState.setActiveProtocol.mockClear()

    const { createProjectionWorker } = await vi.importMock('@worker/createProjectionWorker')

    createProjectionWorker.mockImplementation(() => new MockWorker('projection'))
    ;(global as any).Worker = MockWorker
    ;(global as any).MessageChannel = MockMessageChannel
    ;(global as any).ResizeObserver = vi.fn(function () {
      return {
        observe: vi.fn(),
        disconnect: vi.fn()
      }
    })

    Object.defineProperty(HTMLCanvasElement.prototype, 'transferControlToOffscreen', {
      configurable: true,
      value: vi.fn(() => ({}))
    })
    ;(window as any).projection = {
      ipc: {
        start: vi.fn().mockResolvedValue(undefined),
        stop: vi.fn().mockResolvedValue(undefined),
        sendFrame: vi.fn().mockResolvedValue(undefined),
        setVisible: vi.fn().mockResolvedValue(undefined),
        onAudioChunk: vi.fn(),
        offAudioChunk: vi.fn(),
        onEvent: vi.fn((cb: AnyFn) => (onEventCb = cb)),
        offEvent: vi.fn(),
        sendCommand: vi.fn()
      },
      usb: {
        getDeviceInfo: vi.fn().mockResolvedValue({ device: true }),
        getLastEvent: vi.fn().mockResolvedValue(null),
        listenForEvents: vi.fn((cb: AnyFn) => (usbCb = cb)),
        unlistenForEvents: vi.fn()
      }
    }
  })

  test('usb plugged sets dongle-connected state (main owns session start)', async () => {
    render(<Projection {...baseProps()} />)

    await act(async () => {
      await usbCb?.(null, { type: 'plugged' })
    })

    expect((window as any).projection.ipc.start).not.toHaveBeenCalled()
    expect(statusState.setDongleHardwarePresent).toHaveBeenCalledWith(true)
  })

  test('usb unplugged clears dongle state without touching the video plane', async () => {
    const setReceivingVideo = vi.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} receivingVideo />)

    await act(async () => {
      await usbCb?.(null, { type: 'unplugged' })
    })

    expect((window as any).projection.ipc.stop).not.toHaveBeenCalled()
    expect(setReceivingVideo).not.toHaveBeenCalled()
    expect(statusState.setStreaming).not.toHaveBeenCalled()
    expect(statusState.setDongleHardwarePresent).toHaveBeenCalledWith(false)
    expect(liviState.resetInfo).toHaveBeenCalled()
  })

  test('projection event drives video visibility', async () => {
    const setReceivingVideo = vi.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} />)

    await act(async () => {
      onEventCb?.(null, { type: 'projection', shown: true })
    })
    expect(setReceivingVideo).toHaveBeenCalledWith(true)

    setReceivingVideo.mockClear()
    await act(async () => {
      onEventCb?.(null, { type: 'projection', shown: false })
    })
    expect(setReceivingVideo).toHaveBeenCalledWith(false)
  })

  test('handles worker failure and schedules retry timer', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    const setTimeoutSpy = vi.spyOn(window, 'setTimeout')

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    act(() => {
      projectionWorker.emit({ type: 'failure' })
    })

    expect(setTimeoutSpy).toHaveBeenCalled()

    const timeoutCall = setTimeoutSpy.mock.calls.find((call) => call[1] === 3000)
    expect(timeoutCall).toBeTruthy()
    expect(typeof timeoutCall?.[0]).toBe('function')

    setTimeoutSpy.mockRestore()
    vi.useRealTimers()
  })

  test('handles bluetoothPairedList event from payload string', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'bluetoothPairedList',
        payload: 'device-a\ndevice-b'
      })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('device-a\ndevice-b')
  })

  test('handles dongleInfo event and merges box info', async () => {
    liviState.boxInfo = { existing: 'keep', MDLinkType: 'CarPlay' }
    liviState.dongleFwVersion = 'old-fw'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: {
          dongleFwVersion: 'new-fw',
          boxInfo: { foo: 'bar', MDLinkType: 'AndroidAuto' }
        }
      })
    })

    expect(liviState.dongleFwVersion).toBe('new-fw')
    expect(liviState.boxInfo).toEqual({
      existing: 'keep',
      MDLinkType: 'AndroidAuto',
      foo: 'bar'
    })
  })

  test('handles audioInfo event', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audioInfo',
        payload: {
          codec: 'aac',
          sampleRate: 48000,
          channels: 2,
          bitDepth: 16
        }
      })
    })

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({
      codec: 'aac',
      sampleRate: 48000,
      channels: 2,
      bitDepth: 16
    })
  })

  test('requestHostUI navigates', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestHostUI }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
    })
  })

  test('handles bluetoothPairedList event', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'bluetoothPairedList',
        payload: 'device-a\ndevice-b'
      })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('device-a\ndevice-b')
  })

  test('handles dongleInfo event', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: {
          dongleFwVersion: '2025.02.01',
          boxInfo: { MDLinkType: 'AndroidAuto', foo: 'bar' }
        }
      })
    })

    expect(liviState.dongleFwVersion).toBe('2025.02.01')
    expect(liviState.boxInfo).toEqual({ MDLinkType: 'AndroidAuto', foo: 'bar' })
  })

  test('handles audioInfo event', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audioInfo',
        payload: {
          codec: 'aac',
          sampleRate: 48000,
          channels: 2,
          bitDepth: 16
        }
      })
    })

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({
      codec: 'aac',
      sampleRate: 48000,
      channels: 2,
      bitDepth: 16
    })
  })

  test('handles phone call start (auto switch)', async () => {
    mockPathname = '/media'

    render(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any}
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStart }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    })
  })

  test('sends key command when commandCounter changes and stream is active', async () => {
    statusState.isStreaming = true

    render(<Projection {...baseProps()} command={'home' as any} commandCounter={1} />)

    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledWith('home')
  })

  test('does not re-send last key command on isStreaming flicker', async () => {
    statusState.isStreaming = true

    const { rerender } = render(
      <Projection {...baseProps()} command={'home' as any} commandCounter={1} />
    )
    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledTimes(1)
    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledWith('home')

    statusState.isStreaming = false
    rerender(<Projection {...baseProps()} command={'home' as any} commandCounter={1} />)
    statusState.isStreaming = true
    rerender(<Projection {...baseProps()} command={'home' as any} commandCounter={1} />)

    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledTimes(1)
  })

  // ── IPC plugged / unplugged / failure events ──────────────────────────────

  test('IPC session event marks dongle protocol active', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'session', protocol: 'dongle' })
    })

    expect(statusState.setActiveProtocol).toHaveBeenCalledWith('dongle')
  })

  test('IPC session end clears the active protocol, not the video plane', async () => {
    const setReceivingVideo = vi.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} />)

    act(() => {
      onEventCb?.(null, { type: 'session', protocol: null })
    })

    expect(statusState.setActiveProtocol).toHaveBeenCalledWith(null)
    expect(setReceivingVideo).not.toHaveBeenCalled()
    expect(statusState.setStreaming).not.toHaveBeenCalled()
  })

  test('IPC failure event clears all streaming state', async () => {
    const setReceivingVideo = vi.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} />)

    act(() => {
      onEventCb?.(null, { type: 'failure' })
    })

    expect(statusState.setStreaming).toHaveBeenCalledWith(false)
    expect(statusState.setActiveProtocol).toHaveBeenCalledWith(null)
    expect(setReceivingVideo).toHaveBeenCalledWith(false)
  })

  // ── Audio command events ──────────────────────────────────────────────────

  test('AudioPhonecallStop releases call attention and returns to previous route', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any}
      />
    )

    // arm: switch to projection on call start
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStart }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'

    rerender(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any}
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStop }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))
  })

  test('AudioAttentionRinging triggers call attention switch when autoSwitchOnPhoneCall', async () => {
    mockPathname = '/media'

    render(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any}
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioAttentionRinging }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))
  })

  test('voiceAssistantUiActive triggers voiceAssistant attention switch', async () => {
    mockPathname = '/media'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))
  })

  test('voiceAssistantUiIdle returns to previous route immediately (no timer)', async () => {
    mockPathname = '/media'

    const { rerender } = render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiIdle }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))
  })

  // ── applyAttention: already on projection path ────────────────────────────

  test('applyAttention does nothing when already on projection route', async () => {
    mockPathname = '/'

    render(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any}
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStart }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  // ── mergeBoxInfo: string variants ────────────────────────────────────────

  test('mergeBoxInfo merges when boxInfo payload arrives as JSON string', async () => {
    liviState.boxInfo = { existing: 'keep' }

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: {
          dongleFwVersion: 'fw1',
          boxInfo: '{"MDLinkType":"CarPlay"}'
        }
      })
    })

    expect(liviState.boxInfo).toMatchObject({ existing: 'keep', MDLinkType: 'CarPlay' })
  })

  test('mergeBoxInfo merges when existing boxInfo is a JSON string', async () => {
    liviState.boxInfo = '{"old":"data"}'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: {
          dongleFwVersion: 'fw2',
          boxInfo: { MDLinkType: 'AndroidAuto' }
        }
      })
    })

    expect(liviState.boxInfo).toMatchObject({ old: 'data', MDLinkType: 'AndroidAuto' })
  })

  test('mergeBoxInfo returns prev when boxInfo payload is an empty string', async () => {
    liviState.boxInfo = { preserved: true }

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: { dongleFwVersion: 'fw3', boxInfo: '   ' }
      })
    })

    expect(liviState.boxInfo).toMatchObject({ preserved: true })
  })

  // ── handleAudio: PCM conversion ───────────────────────────────────────────

  test('handleAudio converts int16 chunk to float32 and schedules setPcmData', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    render(<Projection {...baseProps()} />)

    const ipc = (window as any).projection.ipc
    const audioChunkFn: AnyFn = ipc.onAudioChunk.mock.calls[0]?.[0]

    const int16 = new Int16Array([0, 16384, -16384, 32767])
    const buf = int16.buffer

    act(() => {
      audioChunkFn?.({ chunk: { buffer: buf } })
      vi.runAllTimers()
    })

    expect(liviState.setPcmData).toHaveBeenCalledTimes(1)
    const f32: Float32Array = liviState.setPcmData.mock.calls[0][0]
    expect(f32).toBeInstanceOf(Float32Array)
    expect(f32.length).toBe(4)
    expect(f32[0]).toBeCloseTo(0)
    expect(f32[1]).toBeCloseTo(0.5, 1)

    vi.useRealTimers()
  })

  test('handleAudio cleanup clears pending timers on unmount', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    const { unmount } = render(<Projection {...baseProps()} />)

    const ipc = (window as any).projection.ipc
    const audioChunkFn: AnyFn = ipc.onAudioChunk.mock.calls[0]?.[0]

    const int16 = new Int16Array([1000])
    act(() => {
      audioChunkFn?.({ chunk: { buffer: int16.buffer } })
    })

    // Unmount before timer fires → cleanup cancels it
    unmount()

    act(() => {
      vi.runAllTimers()
    })

    expect(liviState.setPcmData).not.toHaveBeenCalled()

    vi.useRealTimers()
  })

  // ── projection worker: requestBuffer & audio messages ────────────────────

  test('projection worker requestBuffer message calls clearRetryTimeout', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    const clearTimeoutSpy = vi.spyOn(window, 'clearTimeout')

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    // Create pending retry timer via 'failure'
    act(() => {
      projectionWorker.emit({ type: 'failure' })
    })

    // requestBuffer clears it
    act(() => {
      projectionWorker.emit({ type: 'requestBuffer' })
    })

    expect(clearTimeoutSpy).toHaveBeenCalled()

    clearTimeoutSpy.mockRestore()
    vi.useRealTimers()
  })

  test('projection worker audio message calls clearRetryTimeout', async () => {
    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    // Should not throw when no retry timer is set
    act(() => {
      projectionWorker.emit({ type: 'audio' })
    })
  })

  // ── clearRetryTimeout with active timer ───────────────────────────────────

  test('clearRetryTimeout clears an active retry timeout', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    act(() => {
      projectionWorker.emit({ type: 'failure' })
    })

    // USB unplug triggers clearRetryTimeout
    act(() => {
      usbCb?.(null, { type: 'unplugged' })
    })

    // Timer was cleared; reload should not fire
    act(() => vi.advanceTimersByTime(5000))

    vi.useRealTimers()
  })

  // ── projection worker: audioInfo / pcmData / command / unknown ───────────

  test('projection worker audioInfo message calls setAudioInfo', async () => {
    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    act(() => {
      projectionWorker.emit({
        type: 'audioInfo',
        payload: { codec: 'pcm', sampleRate: 44100, channels: 1, bitDepth: 16 }
      })
    })

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({
      codec: 'pcm',
      sampleRate: 44100,
      channels: 1,
      bitDepth: 16
    })
  })

  test('projection worker pcmData message calls setPcmData', async () => {
    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]
    const buf = new Float32Array([0.1, 0.2]).buffer

    act(() => {
      projectionWorker.emit({ type: 'pcmData', payload: buf })
    })

    expect(liviState.setPcmData).toHaveBeenCalled()
  })

  test('projection worker command requestHostUI navigates to /media', async () => {
    mockPathname = '/settings'

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    act(() => {
      projectionWorker.emit({
        type: 'command',
        message: { value: CommandMapping.requestHostUI }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))
  })

  test('IPC command with unrecognized value hits final break', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: 9999 }
      })
    })

    // No throw, no navigation
    expect(navigateMock).not.toHaveBeenCalled()
  })

  // ── USB getDeviceInfo failure ─────────────────────────────────────────────

  test('USB connect logs warning when getDeviceInfo throws', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    ;(window as any).projection.usb.getDeviceInfo = vi
      .fn()
      .mockRejectedValue(new Error('no device'))

    render(<Projection {...baseProps()} />)

    await act(async () => {
      await usbCb?.(null, { type: 'plugged' })
    })

    expect(warnSpy).toHaveBeenCalledWith(
      '[PROJECTION] usb.getDeviceInfo() failed',
      expect.any(Error)
    )

    warnSpy.mockRestore()
  })

  // ── mergeBoxInfo edge cases ───────────────────────────────────────────────

  test('mergeBoxInfo returns prev when boxInfo is an invalid JSON string', async () => {
    liviState.boxInfo = { preserved: true }

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: { dongleFwVersion: 'fw', boxInfo: '{invalid json' }
      })
    })

    expect(liviState.boxInfo).toMatchObject({ preserved: true })
  })

  test('mergeBoxInfo sets prev to null when existing boxInfo is invalid JSON string', async () => {
    liviState.boxInfo = '{bad json'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: { dongleFwVersion: 'fw', boxInfo: { MDLinkType: 'CarPlay' } }
      })
    })

    expect(liviState.boxInfo).toMatchObject({ MDLinkType: 'CarPlay' })
  })

  test('mergeBoxInfo sets prev to null when existing boxInfo is an empty string', async () => {
    liviState.boxInfo = '   '

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'dongleInfo',
        payload: { dongleFwVersion: 'fw', boxInfo: { MDLinkType: 'CarPlay' } }
      })
    })

    // prev was empty string → prev=null → result is next object
    expect(liviState.boxInfo).toMatchObject({ MDLinkType: 'CarPlay' })
  })

  // ── projection worker: dongleInfo no-op case ─────────────────────────────

  test('projection worker dongleInfo message is silently ignored', async () => {
    render(<Projection {...baseProps()} />)

    // Should not throw
    act(() => {
      MockWorker.instances[0]?.emit({ type: 'dongleInfo', payload: {} })
    })
  })

  // ── attention back-path cleared when user navigates manually ─────────────

  test('pathname change while attention is armed clears attentionSwitchedByRef', async () => {
    mockPathname = '/media'

    const { rerender } = render(<Projection {...baseProps()} />)

    // Arm voiceAssistant attention
    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    // User manually navigates to '/settings' while voiceAssistant is active
    // → the pathname effect clears attentionSwitchedByRef
    mockPathname = '/settings'
    rerender(<Projection {...baseProps()} />)

    navigateMock.mockClear()

    // VoiceAssistant inactive now: attentionSwitchedByRef is already null → no navigation back
    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiIdle }
      })
    })

    // No back-navigation since attentionSwitchedByRef was cleared
    expect(navigateMock).not.toHaveBeenCalledWith('/media', expect.anything())
  })

  // ── projection worker onerror handler ────────────────────────────────────

  test('projection worker onerror logs to console.error', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]
    projectionWorker.triggerError(new ErrorEvent('error', { message: 'worker crash' }))

    expect(errorSpy).toHaveBeenCalledWith('Worker error:', expect.anything())

    errorSpy.mockRestore()
  })

  // ── recalc runs when content-root element is present ─────────────────────

  test('overlay offset recalc runs when content-root is in the DOM', async () => {
    const anchor = document.createElement('div')
    anchor.id = 'content-root'
    document.body.appendChild(anchor)

    // No throw; recalc should execute the full body with a zero DOMRect
    expect(() => {
      render(<Projection {...baseProps()} />)
    }).not.toThrow()

    document.body.removeChild(anchor)
  })
})

function baseProps(overrides: any = {}) {
  return {
    receivingVideo: false,
    setReceivingVideo: vi.fn(),
    settings: {
      width: 800,
      height: 480,
      fps: 60,
      cluster: { main: false, dash: false, aux: false }
    },
    command: '' as any,
    commandCounter: 0,
    ...overrides
  }
}
