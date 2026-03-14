import { act, render, screen, waitFor } from '@testing-library/react'
import { Maps } from '../Maps'

type AnyFn = (...args: any[]) => any

jest.mock('@worker/workerUrls', () => ({
  renderWorkerUrl: 'render-worker-url'
}))

const statusState: Record<string, any> = {
  isStreaming: true
}
const liviState: Record<string, any> = {
  settings: { fps: 60 },
  boxInfo: null
}

jest.mock('../../../../store/store', () => {
  const useStatusStore: any = (selector: AnyFn) => selector(statusState)
  const useLiviStore: any = (selector: AnyFn) => selector(liviState)
  useLiviStore.setState = (patch: Record<string, any>) => Object.assign(liviState, patch)
  return { useStatusStore, useLiviStore }
})

class MockWorker {
  static instances: MockWorker[] = []
  public postMessage = jest.fn()
  public terminate = jest.fn()
  private listeners: Array<(ev: MessageEvent<any>) => void> = []
  constructor() {
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

describe('Maps page', () => {
  let mapsVideoCb: AnyFn | undefined

  beforeEach(() => {
    MockWorker.instances = []
    MockMessageChannel.instances = []
    mapsVideoCb = undefined

    statusState.isStreaming = true
    liviState.settings = { fps: 60 }
    liviState.boxInfo = { supportFeatures: '' }
    ;(global as any).Worker = MockWorker
    ;(global as any).MessageChannel = MockMessageChannel
    ;(global as any).ResizeObserver = jest.fn(() => ({
      observe: jest.fn(),
      disconnect: jest.fn()
    }))
    ;(global as any).MutationObserver = jest.fn(() => ({
      observe: jest.fn(),
      disconnect: jest.fn()
    }))

    Object.defineProperty(HTMLCanvasElement.prototype, 'transferControlToOffscreen', {
      configurable: true,
      value: jest.fn(() => ({}))
    })

    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    document.body.appendChild(contentRoot)
    ;(window as any).projection = {
      ipc: {
        requestMaps: jest.fn().mockResolvedValue(undefined),
        onMapsVideoChunk: jest.fn((cb: AnyFn) => {
          mapsVideoCb = cb
        })
      }
    }
  })

  test('requests maps stream and initializes render worker', async () => {
    const { unmount } = render(<Maps />)

    await waitFor(() => {
      expect((window as any).projection.ipc.requestMaps).toHaveBeenCalledWith(true)
    })
    expect(MockWorker.instances.length).toBe(1)

    unmount()
    await waitFor(() => {
      expect((window as any).projection.ipc.requestMaps).toHaveBeenCalledWith(false)
    })
  })

  test('forwards video chunks after render-ready', () => {
    render(<Maps />)
    const worker = MockWorker.instances[0]

    act(() => {
      worker.emit({ type: 'render-ready' })
    })

    const buf = new ArrayBuffer(8)
    act(() => {
      mapsVideoCb?.({ chunk: { buffer: buf } })
    })

    const channel = MockMessageChannel.instances[0]
    expect(channel.port1.postMessage).toHaveBeenCalled()
  })

  test('shows renderer error and unsupported firmware hint', () => {
    liviState.boxInfo = { supportFeatures: '' }
    render(<Maps />)

    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-error', message: '' })
    })

    expect(screen.getByText('No renderer available')).toBeInTheDocument()
    expect(screen.getByText('Not supported by firmware')).toBeInTheDocument()
  })
})
