import { act, render, waitFor } from '@testing-library/react'
import { Projection } from '../Projection'
import { CommandMapping } from '@shared/types/ProjectionEnums'

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
  setStreaming: jest.fn((v: boolean) => {
    statusState.isStreaming = v
  }),
  setDongleConnected: jest.fn((v: boolean) => {
    statusState.isDongleConnected = v
  })
}

const liviState: Record<string, any> = {
  negotiatedWidth: 0,
  negotiatedHeight: 0,
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

    const { createProjectionWorker } = jest.requireMock('@worker/createProjectionWorker')
    const { createRenderWorker } = jest.requireMock('@worker/createRenderWorker')

    createProjectionWorker.mockImplementation(() => new MockWorker('projection-worker'))
    createRenderWorker.mockImplementation(() => new MockWorker('render-worker'))

    onEventCb = undefined
    usbCb = undefined
    navigateMock.mockReset()
    mockPathname = '/'

    statusState.isStreaming = true
    statusState.isDongleConnected = true
    statusState.setStreaming.mockClear()
    statusState.setDongleConnected.mockClear()
    liviState.negotiatedWidth = 0
    liviState.negotiatedHeight = 0
    liviState.resetInfo.mockClear()
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

    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    Object.defineProperty(contentRoot, 'getBoundingClientRect', {
      value: () => ({ left: 0, top: 0, width: 1000, height: 700 })
    })
    document.body.appendChild(contentRoot)
    ;(window as any).projection = {
      ipc: {
        start: jest.fn().mockResolvedValue(undefined),
        stop: jest.fn().mockResolvedValue(undefined),
        sendFrame: jest.fn().mockResolvedValue(undefined),
        onVideoChunk: jest.fn(),
        offVideoChunk: jest.fn(),
        onAudioChunk: jest.fn(),
        offAudioChunk: jest.fn(),
        onEvent: jest.fn((cb: AnyFn) => {
          onEventCb = cb
        }),
        offEvent: jest.fn(),
        sendCommand: jest.fn(),
        requestMaps: jest.fn()
      },
      usb: {
        getDeviceInfo: jest.fn().mockResolvedValue({ vendorId: 1, productId: 2, device: true }),
        getLastEvent: jest.fn().mockResolvedValue(null),
        listenForEvents: jest.fn((cb: AnyFn) => {
          usbCb = cb
        }),
        unlistenForEvents: jest.fn()
      }
    }
  })

  test('starts projection on usb plugged and initializes workers', async () => {
    render(
      <Projection
        receivingVideo={false}
        setReceivingVideo={jest.fn()}
        settings={{ width: 800, height: 480, fps: 60, mapsEnabled: false } as any}
        command={'' as any}
        commandCounter={0}
        navVideoOverlayActive={false}
        setNavVideoOverlayActive={jest.fn()}
      />
    )

    expect((window as any).projection.ipc.start).not.toHaveBeenCalled()

    await act(async () => {
      await usbCb?.(null, { type: 'plugged' })
    })

    await waitFor(() => {
      expect((window as any).projection.ipc.start).toHaveBeenCalledTimes(1)
    })
    expect(MockWorker.instances.length).toBeGreaterThanOrEqual(2)
  })

  test('handles render worker errors and usb unplug events', async () => {
    const setReceivingVideo = jest.fn()
    render(
      <Projection
        receivingVideo={true}
        setReceivingVideo={setReceivingVideo}
        settings={{ width: 800, height: 480, fps: 60, mapsEnabled: false } as any}
        command={'' as any}
        commandCounter={0}
        navVideoOverlayActive={false}
        setNavVideoOverlayActive={jest.fn()}
      />
    )

    const renderWorker = MockWorker.instances[1]
    act(() => {
      renderWorker.emit({ type: 'render-error', message: 'No renderer' })
    })
    expect(setReceivingVideo).toHaveBeenCalledWith(false)

    await act(async () => {
      await usbCb?.(null, { type: 'unplugged' })
    })
    expect((window as any).projection.ipc.stop).toHaveBeenCalled()
    expect(statusState.setDongleConnected).toHaveBeenCalledWith(false)
  })

  test('reacts to projection events (resolution and host-ui command)', async () => {
    const setReceivingVideo = jest.fn()
    render(
      <Projection
        receivingVideo={false}
        setReceivingVideo={setReceivingVideo}
        settings={{ width: 800, height: 480, fps: 60, mapsEnabled: true } as any}
        command={'' as any}
        commandCounter={0}
        navVideoOverlayActive={false}
        setNavVideoOverlayActive={jest.fn()}
      />
    )

    act(() => {
      onEventCb?.(null, { type: 'resolution', payload: { width: 1280, height: 720 } })
    })
    expect(setReceivingVideo).toHaveBeenCalledWith(true)

    act(() => {
      onEventCb?.(null, { type: 'command', message: { value: CommandMapping.requestHostUI } })
    })

    await waitFor(() => {
      expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
    })
  })
})
