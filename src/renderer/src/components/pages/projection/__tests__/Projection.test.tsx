import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import { act, render, waitFor } from '@testing-library/react'
import { Projection } from '../Projection'

const navigateMock = jest.fn()
let mockPathname = '/'

jest.mock('@worker/createProjectionWorker', () => ({
  createProjectionWorker: jest.fn()
}))

jest.mock('@worker/createRenderWorker', () => ({
  createRenderWorker: jest.fn()
}))

type AnyFn = (...args: any[]) => any

const statusState: Record<string, any> = {
  isStreaming: true,
  isDongleConnected: true,
  setStreaming: jest.fn(),
  setDongleConnected: jest.fn()
}

const liviState: Record<string, any> = {
  negotiatedWidth: 0,
  negotiatedHeight: 0,
  dongleFwVersion: '',
  boxInfo: null,
  resetInfo: jest.fn(),
  setDeviceInfo: jest.fn(),
  setAudioInfo: jest.fn(),
  setPcmData: jest.fn(),
  setBluetoothPairedList: jest.fn()
}

jest.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useLocation: () => ({ pathname: mockPathname })
}))

jest.mock('../../../../store/store', () => {
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

  return { useStatusStore, useLiviStore }
})

jest.mock('../hooks/useCarplayTouch', () => ({
  useCarplayMultiTouch: () => ({})
}))

class MockWorker {
  static instances: MockWorker[] = []
  public postMessage = jest.fn()
  public terminate = jest.fn()
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
  port1 = { postMessage: jest.fn() }
  port2 = {}
  constructor() {
    MockMessageChannel.instances.push(this)
  }
}

describe('Projection page', () => {
  let onEventCb: AnyFn | undefined
  let usbCb: AnyFn | undefined

  beforeEach(() => {
    MockWorker.instances = []
    MockMessageChannel.instances = []
    navigateMock.mockReset()
    mockPathname = '/'

    statusState.isStreaming = true
    statusState.isDongleConnected = true
    statusState.setStreaming.mockClear()
    statusState.setDongleConnected.mockClear()

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
    statusState.setDongleConnected.mockClear()

    const { createProjectionWorker } = jest.requireMock('@worker/createProjectionWorker')
    const { createRenderWorker } = jest.requireMock('@worker/createRenderWorker')

    createProjectionWorker.mockImplementation(() => new MockWorker('projection'))
    createRenderWorker.mockImplementation(() => new MockWorker('render'))
    ;(global as any).Worker = MockWorker
    ;(global as any).MessageChannel = MockMessageChannel
    ;(global as any).ResizeObserver = jest.fn(() => ({
      observe: jest.fn(),
      disconnect: jest.fn()
    }))

    Object.defineProperty(HTMLCanvasElement.prototype, 'transferControlToOffscreen', {
      configurable: true,
      value: jest.fn(() => ({}))
    })
    ;(window as any).projection = {
      ipc: {
        start: jest.fn().mockResolvedValue(undefined),
        stop: jest.fn().mockResolvedValue(undefined),
        sendFrame: jest.fn().mockResolvedValue(undefined),
        onVideoChunk: jest.fn(),
        offVideoChunk: jest.fn(),
        onAudioChunk: jest.fn(),
        offAudioChunk: jest.fn(),
        onEvent: jest.fn((cb: AnyFn) => (onEventCb = cb)),
        offEvent: jest.fn(),
        sendCommand: jest.fn()
      },
      usb: {
        getDeviceInfo: jest.fn().mockResolvedValue({ device: true }),
        getLastEvent: jest.fn().mockResolvedValue(null),
        listenForEvents: jest.fn((cb: AnyFn) => (usbCb = cb)),
        unlistenForEvents: jest.fn()
      }
    }
  })

  test('usb plugged starts projection', async () => {
    render(<Projection {...baseProps()} />)

    await act(async () => {
      await usbCb?.(null, { type: 'plugged' })
    })

    expect((window as any).projection.ipc.start).toHaveBeenCalled()
  })

  test('usb unplugged stops projection and clears streaming state', async () => {
    const setReceivingVideo = jest.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} receivingVideo />)

    await act(async () => {
      await usbCb?.(null, { type: 'unplugged' })
    })

    expect((window as any).projection.ipc.stop).toHaveBeenCalled()
    expect(setReceivingVideo).toHaveBeenCalledWith(false)
    expect(statusState.setStreaming).toHaveBeenCalledWith(false)
    expect(statusState.setDongleConnected).toHaveBeenCalledWith(false)
    expect(liviState.resetInfo).toHaveBeenCalled()
  })

  test('handles render-ready', () => {
    render(<Projection {...baseProps()} />)

    const renderWorker = MockWorker.instances[1]

    act(() => {
      renderWorker.emit({ type: 'render-ready' })
    })

    expect(renderWorker.postMessage).not.toHaveBeenCalledWith({ type: 'clear' })
  })

  test('handles render-error', () => {
    const setReceivingVideo = jest.fn()

    render(<Projection {...baseProps({ setReceivingVideo })} receivingVideo />)

    const renderWorker = MockWorker.instances[1]

    act(() => {
      renderWorker.emit({ type: 'render-error', message: 'fail' })
    })

    expect(setReceivingVideo).toHaveBeenCalledWith(false)
  })

  test('forces video hidden and clears render worker when streaming becomes false', () => {
    const setReceivingVideo = jest.fn()

    const { rerender } = render(<Projection {...baseProps({ setReceivingVideo })} receivingVideo />)

    const renderWorker = MockWorker.instances[1]

    statusState.isStreaming = false

    rerender(<Projection {...baseProps({ setReceivingVideo })} receivingVideo />)

    expect(setReceivingVideo).toHaveBeenCalledWith(false)
    expect(renderWorker.postMessage).toHaveBeenCalledWith({ type: 'clear' })
  })

  test('handles worker failure and schedules retry timer', () => {
    jest.useFakeTimers()

    const setTimeoutSpy = jest.spyOn(window, 'setTimeout')

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
    jest.useRealTimers()
  })

  test('handles bluetoothPairedList event from payload string', () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'bluetoothPairedList',
        payload: 'device-a\ndevice-b'
      })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('device-a\ndevice-b')
  })

  test('handles dongleInfo event and merges box info', () => {
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

  test('handles audioInfo event', () => {
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

  test('handles bluetoothPairedList event', () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'bluetoothPairedList',
        payload: 'device-a\ndevice-b'
      })
    })

    expect(liviState.setBluetoothPairedList).toHaveBeenCalledWith('device-a\ndevice-b')
  })

  test('handles dongleInfo event', () => {
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

  test('handles audioInfo event', () => {
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

  test('requestVideoFocus navigates to projection', async () => {
    mockPathname = '/media'

    render(
      <Projection
        {...baseProps()}
        settings={
          { width: 800, height: 480, fps: 60, mapsEnabled: false, autoSwitchOnStream: true } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    })
  })

  test('requestVideoFocus waits for resolution when stream is not active', async () => {
    mockPathname = '/media'
    statusState.isStreaming = false

    const setReceivingVideo = jest.fn()

    render(
      <Projection
        {...baseProps({ setReceivingVideo })}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()

    act(() => {
      onEventCb?.(null, {
        type: 'resolution',
        payload: { width: 1280, height: 720 }
      })
    })

    expect(setReceivingVideo).toHaveBeenCalledWith(true)

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    })
  })

  test('releaseVideoFocus navigates back after auto switch', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    })

    navigateMock.mockClear()
    mockPathname = '/'

    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseVideoFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
    })
  })

  test('releaseVideoFocus does nothing when auto switch on stream is disabled', () => {
    mockPathname = '/'

    render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: false
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseVideoFocus }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('requestNaviFocus shows overlay when maps disabled', () => {
    mockPathname = '/media'

    const setNavVideoOverlayActive = jest.fn()

    render(
      <Projection
        {...baseProps({ setNavVideoOverlayActive })}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestNaviFocus }
      })
    })

    expect(setNavVideoOverlayActive).toHaveBeenCalledWith(true)
  })

  test('releaseNaviFocus navigates back from maps when maps are enabled', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestNaviFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/maps', { replace: true })
    })

    navigateMock.mockClear()
    mockPathname = '/maps'

    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseNaviFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
    })
  })

  test('releaseNaviFocus navigates back from maps when maps are enabled', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestNaviFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/maps', { replace: true })
    })

    navigateMock.mockClear()
    mockPathname = '/maps'

    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseNaviFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
    })
  })

  test('releaseVideoFocus navigates back after auto switch', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    })

    navigateMock.mockClear()
    mockPathname = '/'

    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseVideoFocus }
      })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
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

  test('sends key command when commandCounter changes and stream is active', () => {
    statusState.isStreaming = true

    render(<Projection {...baseProps()} command={'home' as any} commandCounter={1} />)

    expect((window as any).projection.ipc.sendCommand).toHaveBeenCalledWith('home')
  })

  test('does not send key command when stream is inactive', () => {
    statusState.isStreaming = false

    render(<Projection {...baseProps()} command={'home' as any} commandCounter={1} />)

    expect((window as any).projection.ipc.sendCommand).not.toHaveBeenCalled()
  })

  // ── IPC plugged / unplugged / failure events ──────────────────────────────

  test('IPC plugged event marks dongle connected', () => {
    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, { type: 'plugged' })
    })

    expect(statusState.setDongleConnected).toHaveBeenCalledWith(true)
  })

  test('IPC unplugged event clears all streaming state', () => {
    const setReceivingVideo = jest.fn()
    const setNavVideoOverlayActive = jest.fn()

    render(<Projection {...baseProps({ setReceivingVideo, setNavVideoOverlayActive })} />)

    act(() => {
      onEventCb?.(null, { type: 'unplugged' })
    })

    expect(statusState.setStreaming).toHaveBeenCalledWith(false)
    expect(statusState.setDongleConnected).toHaveBeenCalledWith(false)
    expect(setReceivingVideo).toHaveBeenCalledWith(false)
    expect(setNavVideoOverlayActive).toHaveBeenCalledWith(false)
  })

  test('IPC failure event clears all streaming state', () => {
    const setReceivingVideo = jest.fn()
    const setNavVideoOverlayActive = jest.fn()

    render(<Projection {...baseProps({ setReceivingVideo, setNavVideoOverlayActive })} />)

    act(() => {
      onEventCb?.(null, { type: 'failure' })
    })

    expect(statusState.setStreaming).toHaveBeenCalledWith(false)
    expect(statusState.setDongleConnected).toHaveBeenCalledWith(false)
    expect(setReceivingVideo).toHaveBeenCalledWith(false)
    expect(setNavVideoOverlayActive).toHaveBeenCalledWith(false)
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

  test('AudioVoiceAssistantStart triggers voiceAssistant attention switch', async () => {
    mockPathname = '/media'

    render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))
  })

  test('AudioVoiceAssistantStop returns to previous route via debounce timer', async () => {
    jest.useFakeTimers()
    mockPathname = '/media'

    const { rerender } = render(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStop }
      })
    })

    // timer not yet fired
    expect(navigateMock).not.toHaveBeenCalled()

    act(() => {
      jest.advanceTimersByTime(200)
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))

    jest.useRealTimers()
  })

  // ── applyAttention: already on projection path ────────────────────────────

  test('applyAttention does nothing when already on projection route', () => {
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

  // ── clearVoiceAssistantReleaseTimer when timer is already set ─────────────

  test('clearVoiceAssistantReleaseTimer cancels pending debounce on second active', async () => {
    jest.useFakeTimers()
    mockPathname = '/media'

    const { rerender } = render(<Projection {...baseProps()} />)

    // First voiceAssistant start → switch to projection
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(<Projection {...baseProps()} />)

    // VoiceAssistant stop → sets debounce timer
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStop }
      })
    })

    // VoiceAssistant start again before timer fires → clearVoiceAssistantReleaseTimer runs with timer set
    mockPathname = '/'
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
      })
    })

    // Timer should have been cancelled, so no navigation after advance
    act(() => jest.advanceTimersByTime(200))
    expect(navigateMock).not.toHaveBeenCalledWith('/media', expect.anything())

    jest.useRealTimers()
  })

  // ── mergeBoxInfo: string variants ────────────────────────────────────────

  test('mergeBoxInfo merges when boxInfo payload arrives as JSON string', () => {
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

  test('mergeBoxInfo merges when existing boxInfo is a JSON string', () => {
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

  test('mergeBoxInfo returns prev when boxInfo payload is an empty string', () => {
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

  // ── handleVideo: all branches ─────────────────────────────────────────────

  test('handleVideo forwards valid buffer to video channel port', () => {
    render(<Projection {...baseProps()} />)

    const ipc = (window as any).projection.ipc

    // Trigger render-ready → new handleVideo closure registered
    act(() => {
      MockWorker.instances[1]?.emit({ type: 'render-ready' })
    })

    // Get the latest registered callback (re-registered after render-ready)
    const videoChunkFn: AnyFn = ipc.onVideoChunk.mock.calls.at(-1)?.[0]

    const buf = new ArrayBuffer(8)

    act(() => {
      videoChunkFn?.({ chunk: { buffer: buf } })
    })

    // videoChannel.port1 is MockMessageChannel.instances[0].port1 (video channel is first)
    const port1 = MockMessageChannel.instances[0]?.port1
    expect(port1?.postMessage).toHaveBeenCalledWith(buf, [buf])
  })

  test('handleVideo skips when rendererError is set', () => {
    const setReceivingVideo = jest.fn()
    render(<Projection {...baseProps({ setReceivingVideo })} />)

    const ipc = (window as any).projection.ipc

    // Trigger render-error → rendererError state set, new handleVideo registered
    act(() => {
      MockWorker.instances[1]?.emit({ type: 'render-error', message: 'gpu fail' })
    })

    const videoChunkFn: AnyFn = ipc.onVideoChunk.mock.calls.at(-1)?.[0]
    const port1 = MockMessageChannel.instances[0]?.port1
    const callsBefore = port1?.postMessage.mock.calls.length ?? 0

    act(() => {
      videoChunkFn?.({ chunk: { buffer: new ArrayBuffer(4) } })
    })

    expect(port1?.postMessage.mock.calls.length).toBe(callsBefore)
  })

  test('handleVideo ignores non-object payload', () => {
    render(<Projection {...baseProps()} />)

    const ipc = (window as any).projection.ipc
    act(() => {
      MockWorker.instances[1]?.emit({ type: 'render-ready' })
    })

    const videoChunkFn: AnyFn = ipc.onVideoChunk.mock.calls.at(-1)?.[0]
    const port1 = MockMessageChannel.instances[0]?.port1

    act(() => {
      videoChunkFn?.(null)
      videoChunkFn?.('string')
      videoChunkFn?.({ chunk: {} }) // object but no buffer
    })

    expect(port1?.postMessage).not.toHaveBeenCalled()
  })

  // ── handleAudio: PCM conversion ───────────────────────────────────────────

  test('handleAudio converts int16 chunk to float32 and schedules setPcmData', () => {
    jest.useFakeTimers()

    render(<Projection {...baseProps()} />)

    const ipc = (window as any).projection.ipc
    const audioChunkFn: AnyFn = ipc.onAudioChunk.mock.calls[0]?.[0]

    const int16 = new Int16Array([0, 16384, -16384, 32767])
    const buf = int16.buffer

    act(() => {
      audioChunkFn?.({ chunk: { buffer: buf } })
      jest.runAllTimers()
    })

    expect(liviState.setPcmData).toHaveBeenCalledTimes(1)
    const f32: Float32Array = liviState.setPcmData.mock.calls[0][0]
    expect(f32).toBeInstanceOf(Float32Array)
    expect(f32.length).toBe(4)
    expect(f32[0]).toBeCloseTo(0)
    expect(f32[1]).toBeCloseTo(0.5, 1)

    jest.useRealTimers()
  })

  test('handleAudio cleanup clears pending timers on unmount', () => {
    jest.useFakeTimers()

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
      jest.runAllTimers()
    })

    expect(liviState.setPcmData).not.toHaveBeenCalled()

    jest.useRealTimers()
  })

  // ── projection worker: requestBuffer & audio messages ────────────────────

  test('projection worker requestBuffer message calls clearRetryTimeout', () => {
    jest.useFakeTimers()

    const clearTimeoutSpy = jest.spyOn(window, 'clearTimeout')

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
    jest.useRealTimers()
  })

  test('projection worker audio message calls clearRetryTimeout', () => {
    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]

    // Should not throw when no retry timer is set
    act(() => {
      projectionWorker.emit({ type: 'audio' })
    })
  })

  // ── clearRetryTimeout with active timer ───────────────────────────────────

  test('clearRetryTimeout clears an active retry timeout', () => {
    jest.useFakeTimers()

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
    act(() => jest.advanceTimersByTime(5000))

    jest.useRealTimers()
  })

  // ── requestVideoFocus blocked by attention ────────────────────────────────

  test('requestVideoFocus does not auto-switch while attention (voiceAssistant) is active', async () => {
    mockPathname = '/media'

    render(
      <Projection
        {...baseProps()}
        settings={{ width: 800, height: 480, fps: 60, autoSwitchOnStream: true } as any}
      />
    )

    // Arm voiceAssistant attention (switches to projection)
    act(() => {
      onEventCb?.(null, {
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    navigateMock.mockClear()

    // requestVideoFocus while voiceAssistant attention is active → blocked
    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })

    expect(navigateMock).not.toHaveBeenCalled()
  })

  // ── releaseNaviFocus with mapsEnabled=false dismisses overlay ────────────

  test('releaseNaviFocus with mapsEnabled=false calls setNavVideoOverlayActive(false)', () => {
    mockPathname = '/media'
    const setNavVideoOverlayActive = jest.fn()

    render(
      <Projection
        {...baseProps({ setNavVideoOverlayActive })}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnGuidance: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseNaviFocus }
      })
    })

    expect(setNavVideoOverlayActive).toHaveBeenCalledWith(false)
  })

  // ── releaseVideoFocus: maps back-navigation ───────────────────────────────

  test('releaseVideoFocus with mapsEnabled navigates back from maps via lastNonMapsPathRef', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    // requestNaviFocus stores lastNonMapsPathRef = '/media' and navigates to '/maps'
    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestNaviFocus }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/maps', { replace: true }))

    navigateMock.mockClear()
    mockPathname = '/maps'

    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: true,
            autoSwitchOnGuidance: true,
            autoSwitchOnStream: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseVideoFocus }
      })
    })

    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true }))
  })

  // ── releaseVideoFocus: blocked by attention ───────────────────────────────

  test('releaseVideoFocus does not navigate when attention switch is active', async () => {
    mockPathname = '/media'

    const { rerender } = render(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true,
            autoSwitchOnPhoneCall: true
          } as any
        }
      />
    )

    // requestVideoFocus: auto-switch
    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.requestVideoFocus }
      })
    })
    await waitFor(() => expect(navigateMock).toHaveBeenCalledWith('/', { replace: true }))

    // call attention fires on top of that
    act(() => {
      onEventCb?.(null, { type: 'audio', payload: { command: AudioCommand.AudioPhonecallStart } })
    })

    navigateMock.mockClear()
    mockPathname = '/'
    rerender(
      <Projection
        {...baseProps()}
        settings={
          {
            width: 800,
            height: 480,
            fps: 60,
            mapsEnabled: false,
            autoSwitchOnStream: true,
            autoSwitchOnPhoneCall: true
          } as any
        }
      />
    )

    act(() => {
      onEventCb?.(null, {
        type: 'command',
        message: { value: CommandMapping.releaseVideoFocus }
      })
    })

    expect(navigateMock).not.toHaveBeenCalledWith('/media', expect.anything())
  })

  // ── projection worker: audioInfo / pcmData / command / unknown ───────────

  test('projection worker audioInfo message calls setAudioInfo', () => {
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

  test('projection worker pcmData message calls setPcmData', () => {
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

  test('IPC command with unrecognized value hits final break', () => {
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
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

    ;(window as any).projection.usb.getDeviceInfo = jest
      .fn()
      .mockRejectedValue(new Error('no device'))

    render(<Projection {...baseProps()} />)

    await act(async () => {
      await usbCb?.(null, { type: 'plugged' })
    })

    expect(warnSpy).toHaveBeenCalledWith('[CARPLAY] usb.getDeviceInfo() failed', expect.any(Error))

    warnSpy.mockRestore()
  })

  // ── mergeBoxInfo edge cases ───────────────────────────────────────────────

  test('mergeBoxInfo returns prev when boxInfo is an invalid JSON string', () => {
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

  test('mergeBoxInfo sets prev to null when existing boxInfo is invalid JSON string', () => {
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

  test('mergeBoxInfo sets prev to null when existing boxInfo is an empty string', () => {
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

  test('projection worker dongleInfo message is silently ignored', () => {
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
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStart }
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
        type: 'audio',
        payload: { command: AudioCommand.AudioVoiceAssistantStop }
      })
    })

    // No back-navigation since attentionSwitchedByRef was cleared
    expect(navigateMock).not.toHaveBeenCalledWith('/media', expect.anything())
  })

  // ── projection worker onerror handler ────────────────────────────────────

  test('projection worker onerror logs to console.error', () => {
    const errorSpy = jest.spyOn(console, 'error').mockImplementation(() => {})

    render(<Projection {...baseProps()} />)

    const projectionWorker = MockWorker.instances[0]
    projectionWorker.triggerError(new ErrorEvent('error', { message: 'worker crash' }))

    expect(errorSpy).toHaveBeenCalledWith('Worker error:', expect.anything())

    errorSpy.mockRestore()
  })

  // ── recalc runs when content-root element is present ─────────────────────

  test('overlay offset recalc runs when content-root is in the DOM', () => {
    const anchor = document.createElement('div')
    anchor.id = 'content-root'
    document.body.appendChild(anchor)

    // No throw; recalc should execute the full body with a zero DOMRect
    expect(() => {
      render(<Projection {...baseProps()} />)
    }).not.toThrow()

    document.body.removeChild(anchor)
  })

  // ── navVideoOverlayActive pointerdown dismiss ─────────────────────────────

  test('navVideoOverlayActive pointerdown dismisses overlay', () => {
    mockPathname = '/media'
    const setNavVideoOverlayActive = jest.fn()

    render(<Projection {...baseProps({ setNavVideoOverlayActive })} navVideoOverlayActive={true} />)

    act(() => {
      const evt = document.createEvent('Event')
      evt.initEvent('pointerdown', true, true)
      window.dispatchEvent(evt)
    })

    expect(setNavVideoOverlayActive).toHaveBeenCalledWith(false)
  })
})

function baseProps(overrides: any = {}) {
  return {
    receivingVideo: false,
    setReceivingVideo: jest.fn(),
    settings: { width: 800, height: 480, fps: 60, mapsEnabled: false },
    command: '' as any,
    commandCounter: 0,
    navVideoOverlayActive: false,
    setNavVideoOverlayActive: jest.fn(),
    ...overrides
  }
}
