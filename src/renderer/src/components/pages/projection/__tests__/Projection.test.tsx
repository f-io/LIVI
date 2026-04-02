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
