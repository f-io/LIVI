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
  activeProtocol: null,
  setStreaming: vi.fn(),
  setActiveProtocol: vi.fn()
}

const liviState: Record<string, any> = {
  negotiatedWidth: 0,
  negotiatedHeight: 0,
  resetInfo: vi.fn(),
  setDeviceInfo: vi.fn(),
  setAudioInfo: vi.fn(),
  setPcmData: vi.fn(),
  setBluetoothPairedList: vi.fn(),
  bumpAudioDevicesRevision: vi.fn()
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

  const useProjectionActive = () => statusState.activeProtocol != null

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
    statusState.activeProtocol = null
    statusState.setStreaming.mockClear()
    statusState.setActiveProtocol.mockClear()

    liviState.negotiatedWidth = 0
    liviState.negotiatedHeight = 0
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
    liviState.bumpAudioDevicesRevision.mockClear()
    statusState.setStreaming.mockClear()
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

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({ sampleRate: 48000 })
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

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({ sampleRate: 48000 })
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

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({ sampleRate: 44100 })
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

  // ── mergeBoxInfo edge cases ───────────────────────────────────────────────

  // ── projection worker: dongleInfo no-op case ─────────────────────────────

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

  test('navigating back to projection presses home and requests a frame', async () => {
    mockPathname = '/media'
    statusState.activeProtocol = 'carplay'

    const { rerender } = render(<Projection {...baseProps()} />)
    ;(window as any).projection.ipc.sendCommand.mockClear()
    ;(window as any).projection.ipc.sendFrame.mockClear()

    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    await waitFor(() =>
      expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledWith('home')
    )
    expect((window as any).projection.ipc.sendFrame).toHaveBeenCalled()
  })

  test('navigating back to projection is a no-op while projection is inactive', async () => {
    statusState.activeProtocol = null

    mockPathname = '/media'
    const { rerender } = render(<Projection {...baseProps()} />)
    ;(window as any).projection.ipc.sendCommand.mockClear()

    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    expect((window as any).projection.ipc.sendCommand).not.toHaveBeenCalledWith('home')
  })

  test('back-to-projection frame request swallows a rejection', async () => {
    ;(window as any).projection.ipc.sendFrame = vi.fn().mockRejectedValue(new Error('no frame'))

    mockPathname = '/media'
    statusState.activeProtocol = 'carplay'
    const { rerender } = render(<Projection {...baseProps()} />)

    mockPathname = '/'
    expect(() => rerender(<Projection {...baseProps()} />)).not.toThrow()

    await waitFor(() =>
      expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledWith('home')
    )
  })

  test('visibility update swallows a rejection', async () => {
    ;(window as any).projection.ipc.setVisible = vi.fn().mockRejectedValue(new Error('hidden'))

    expect(() => render(<Projection {...baseProps()} />)).not.toThrow()

    await waitFor(() => expect((window as any).projection.ipc.setVisible).toHaveBeenCalled())
  })

  test('overlay recalc tolerates a missing ResizeObserver', () => {
    const anchor = document.createElement('div')
    anchor.id = 'content-root'
    document.body.appendChild(anchor)

    const { rerender } = render(
      <Projection {...baseProps({ settings: baseSettings({ hand: 'left' }) })} />
    )

    const original = (global as any).ResizeObserver
    ;(global as any).ResizeObserver = undefined

    expect(() =>
      rerender(<Projection {...baseProps({ settings: baseSettings({ hand: 'right' }) })} />)
    ).not.toThrow()

    ;(global as any).ResizeObserver = original
    document.body.removeChild(anchor)
  })

  test('worker audioInfo without payload defaults sample rate to zero', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({ type: 'audioInfo' })
    })

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({ sampleRate: 0 })
  })

  test('worker command voiceAssistantUiActive switches to projection', async () => {
    mockPathname = '/media'

    render(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))
  })

  test('worker command voiceAssistantUiIdle returns to the previous route', async () => {
    mockPathname = '/media'

    const { rerender } = render(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiIdle }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))
  })

  test('worker failure retry timer reloads the window', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })

    const reloadSpy = vi.fn()
    const originalLocation = window.location
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: { ...originalLocation, reload: reloadSpy }
    })

    render(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({ type: 'failure' })
    })

    act(() => {
      vi.advanceTimersByTime(3000)
    })

    expect(reloadSpy).toHaveBeenCalled()

    Object.defineProperty(window, 'location', {
      configurable: true,
      value: originalLocation
    })
    vi.useRealTimers()
  })

  test('bluetoothPairedList reads a nested payload.data string', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'bluetoothPairedList', payload: { data: 'nested-a\nnested-b' } })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('nested-a\nnested-b')
  })

  test('bluetoothPairedList reads a top-level data string', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'bluetoothPairedList', data: 'top-a\ntop-b' })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('top-a\ntop-b')
  })

  test('bluetoothPairedList falls back to an empty string', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'bluetoothPairedList' })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('')
  })

  test('audioDevicesChanged bumps the audio devices revision', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'audioDevicesChanged' })
    })

    expect(liviState.bumpAudioDevicesRevision).toHaveBeenCalled()
  })

  test('resolution event stores negotiated dimensions', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'resolution', payload: { width: 1280, height: 720 } })
    })

    expect(liviState.negotiatedWidth).toBe(1280)
    expect(liviState.negotiatedHeight).toBe(720)
  })

  test('resolution event without a payload is ignored', async () => {
    liviState.negotiatedWidth = 42

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'resolution' })
    })

    expect(liviState.negotiatedWidth).toBe(42)
  })

  test('resolution event with a non-numeric width is ignored', async () => {
    liviState.negotiatedWidth = 43

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'resolution', payload: { width: 'x', height: 100 } })
    })

    expect(liviState.negotiatedWidth).toBe(43)
  })

  test('resolution event with a missing height is ignored', async () => {
    liviState.negotiatedWidth = 44

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'resolution', payload: { width: 100 } })
    })

    expect(liviState.negotiatedWidth).toBe(44)
  })

  test('audio event with a non-numeric command is ignored', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'audio', payload: {} })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('ipc audioInfo without a payload is ignored', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'audioInfo' })
    })

    expect(liviState.setAudioInfo).not.toHaveBeenCalled()
  })

  test('ipc audioInfo without a sample rate defaults to zero', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'audioInfo', payload: {} })
    })

    expect(liviState.setAudioInfo).toHaveBeenCalledWith({ sampleRate: 0 })
  })

  test('ipc command with a non-numeric value is ignored', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'command', message: {} })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('resize observer forwards frame requests to the worker', async () => {
    let roCb: (() => void) | undefined
    ;(global as any).ResizeObserver = vi.fn(function (cb: () => void) {
      roCb = cb
      return { observe: vi.fn(), disconnect: vi.fn() }
    })

    render(<Projection {...baseProps()} />)

    const worker = MockWorker.instances[0]
    worker.postMessage.mockClear()

    act(() => {
      roCb?.()
    })

    expect(worker.postMessage).toHaveBeenCalledWith({ type: 'frame' })
  })

  test('key command effect skips when the counter matches the last sent value', async () => {
    const { rerender } = render(
      <Projection {...baseProps()} command={'home' as any} commandCounter={1} />
    )
    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledTimes(1)

    rerender(<Projection {...baseProps()} command={'back' as any} commandCounter={1} />)

    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledTimes(1)
  })

  test('computes the touch transform when dimensions are negotiated', () => {
    liviState.negotiatedWidth = 1920
    liviState.negotiatedHeight = 1080

    expect(() =>
      render(
        <Projection
          {...baseProps({
            settings: baseSettings({ projectionWidth: 800, projectionHeight: 480 })
          })}
        />
      )
    ).not.toThrow()
  })

  test('handles null negotiated dimensions', () => {
    liviState.negotiatedWidth = null
    liviState.negotiatedHeight = null

    expect(() => render(<Projection {...baseProps()} />)).not.toThrow()
  })

  test('handles negotiated width without a negotiated height', () => {
    liviState.negotiatedWidth = 100
    liviState.negotiatedHeight = 0

    expect(() => render(<Projection {...baseProps()} />)).not.toThrow()
  })

  test('handles negotiated dimensions with a zero projection width', () => {
    liviState.negotiatedWidth = 100
    liviState.negotiatedHeight = 100

    expect(() =>
      render(
        <Projection
          {...baseProps({ settings: baseSettings({ projectionWidth: 0, projectionHeight: 480 }) })}
        />
      )
    ).not.toThrow()
  })

  test('handles negotiated dimensions with a zero projection height', () => {
    liviState.negotiatedWidth = 100
    liviState.negotiatedHeight = 100

    expect(() =>
      render(
        <Projection
          {...baseProps({ settings: baseSettings({ projectionWidth: 800, projectionHeight: 0 }) })}
        />
      )
    ).not.toThrow()
  })

  test('requestHostUI does not navigate when already on the host route', async () => {
    mockPathname = '/media'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'command', message: { value: CommandMapping.requestHostUI } })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('call attention stays armed when active follows ringing on projection', async () => {
    mockPathname = '/media'
    const settings = baseSettings({ autoSwitchOnPhoneCall: true })

    const { rerender } = render(<Projection {...baseProps({ settings })} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioAttentionRinging }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(<Projection {...baseProps({ settings })} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStart }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('voiceAssistant idle does not navigate back when not on the projection route', async () => {
    mockPathname = '/media'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiActive }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.voiceAssistantUiIdle }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('worker command with an unrecognized value is ignored', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      MockWorker.instances[0]?.emit({ type: 'command', message: { value: 4242 } })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('worker failure does not schedule a second retry while one is pending', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const setTimeoutSpy = vi.spyOn(window, 'setTimeout')

    render(<Projection {...baseProps()} />)

    const worker = MockWorker.instances[0]

    act(() => {
      worker.emit({ type: 'failure' })
    })
    act(() => {
      worker.emit({ type: 'failure' })
    })

    const retryCalls = setTimeoutSpy.mock.calls.filter((call) => call[1] === 3000)
    expect(retryCalls).toHaveLength(1)

    setTimeoutSpy.mockRestore()
    vi.useRealTimers()
  })

  test('audio event with an unrecognized numeric command is ignored', async () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'audio', payload: { command: 99999 } })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  // ── receivingVideo=true: overlay hides, video plane reveals ──────────────

  test('receiving video hides the status overlay and reveals the video plane', () => {
    mockPathname = '/'

    const { container } = render(<Projection {...baseProps({ receivingVideo: true })} />)

    const overlay = container.querySelector('[role="status"]') as HTMLElement
    expect(overlay).toHaveStyle({ display: 'none' })

    const videoContainer = container.querySelector('#videoContainer') as HTMLElement
    expect(videoContainer.style.backgroundColor).toBe('transparent')
    expect(videoContainer.style.visibility).toBe('visible')
    expect(videoContainer.style.zIndex).toBe('1')
  })

  // ── returning to projection while attention already switched us there ────

  test('does not press home when attention already switched us to projection', async () => {
    mockPathname = '/media'
    statusState.activeProtocol = 'carplay'

    const settings = { width: 800, height: 480, fps: 60, autoSwitchOnPhoneCall: true } as any

    const { rerender } = render(<Projection {...baseProps({ settings })} />)

    // Arm call attention (calls navigate('/', ...), attentionSwitchedByRef.current = 'call')
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioPhonecallStart }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    ;(window as any).projection.ipc.sendCommand.mockClear()

    // Pathname actually flips to '/' while attentionSwitchedByRef.current is still 'call' and
    // isProjectionActive stays true: the 'home' press must be skipped so an in-progress
    // Siri/call session isn't dismissed.
    mockPathname = '/'
    rerender(<Projection {...baseProps({ settings })} />)

    expect((window as any).projection.ipc.sendCommand).not.toHaveBeenCalledWith('home')
  })
})

function baseSettings(overrides: any = {}) {
  return {
    width: 800,
    height: 480,
    fps: 60,
    cluster: { main: false, dash: false, aux: false },
    ...overrides
  }
}

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
