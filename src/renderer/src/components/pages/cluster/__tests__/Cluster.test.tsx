import { act, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { Cluster } from '../Cluster'

const renderCluster = (path = '/cluster') =>
  render(
    <MemoryRouter initialEntries={[path]}>
      <Cluster />
    </MemoryRouter>
  )

type AnyFn = (...args: any[]) => any

jest.mock('@worker/createRenderWorker', () => ({
  createRenderWorker: jest.fn()
}))

const statusState: Record<string, any> = {
  isStreaming: true
}
const liviState: Record<string, any> = {
  settings: { fps: 60, clusterFps: 60, cluster: { main: true, dash: false, aux: false } },
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

describe('Cluster page', () => {
  let clusterVideoCb: AnyFn | undefined

  beforeEach(() => {
    const { createRenderWorker } = jest.requireMock('@worker/createRenderWorker')
    createRenderWorker.mockImplementation(() => new MockWorker())

    MockWorker.instances = []
    MockMessageChannel.instances = []
    clusterVideoCb = undefined

    statusState.isStreaming = true
    liviState.settings = {
      fps: 60,
      clusterFps: 60,
      cluster: { main: true, dash: false, aux: false }
    }
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
        requestCluster: jest.fn().mockResolvedValue(undefined),
        onClusterVideoChunk: jest.fn((cb: AnyFn) => {
          clusterVideoCb = cb
        }),
        onEvent: jest.fn(),
        offEvent: jest.fn()
      }
    }
  })

  test('requests cluster stream and initializes render worker', async () => {
    const projectionEventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      projectionEventCbs.push(cb)
    })
    ;(window as any).projection.ipc.offEvent = jest.fn((cb: AnyFn) => {
      const i = projectionEventCbs.indexOf(cb)
      if (i >= 0) projectionEventCbs.splice(i, 1)
    })

    renderCluster()

    await waitFor(() => {
      expect(MockWorker.instances.length).toBe(1)
    })

    // requestCluster(true) is gated on render-ready
    act(() => {
      MockWorker.instances[0].emit({ type: 'render-ready' })
    })

    await waitFor(() => {
      expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(true)
    })

    // Release happens on phone disconnect, not on unmount — the worker is
    // permanently mounted at the App level.
    act(() => {
      projectionEventCbs.forEach((cb) => cb(undefined, { type: 'unplugged' }))
    })
    await waitFor(() => {
      expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(false)
    })
  })

  test('forwards video chunks after render-ready', () => {
    renderCluster()
    const worker = MockWorker.instances[0]

    act(() => {
      worker.emit({ type: 'render-ready' })
    })

    const buf = new ArrayBuffer(8)
    act(() => {
      clusterVideoCb?.({ chunk: { buffer: buf } })
    })

    const channel = MockMessageChannel.instances[0]
    expect(channel.port1.postMessage).toHaveBeenCalled()
  })

  test('does not request the cluster stream when no display targets it', async () => {
    liviState.settings = {
      fps: 60,
      clusterFps: 60,
      cluster: { main: false, dash: false, aux: false }
    }

    renderCluster()

    // Give React a tick to flush effects.
    await Promise.resolve()
    await Promise.resolve()

    // Worker still spawns (same pattern as the main projection worker, so the
    // initial IDR is never missed if cluster gets enabled later), but no
    // requestCluster IPC fires while no display targets it.
    expect((window as any).projection.ipc.requestCluster).not.toHaveBeenCalled()
  })

  test('shows renderer error and unsupported firmware hint', () => {
    liviState.boxInfo = { supportFeatures: '' }
    renderCluster()

    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-error', message: '' })
    })

    expect(screen.getByText('No renderer available')).toBeInTheDocument()
    expect(screen.getByText('Not supported by firmware')).toBeInTheDocument()
  })
})
