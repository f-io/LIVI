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

  beforeAll(() => {
    jest.spyOn(console, 'log').mockImplementation(() => {})
    jest.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterAll(() => jest.restoreAllMocks())

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
        onClusterResolution: jest.fn(),
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

  test('renderer error with an explicit message wins over the fallback', () => {
    renderCluster()
    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-error', message: 'WebGPU exploded' })
    })
    expect(screen.getByText('WebGPU exploded')).toBeInTheDocument()
  })

  test('parseBoxInfo accepts a stringified JSON boxInfo and detects naviScreen', () => {
    liviState.boxInfo = JSON.stringify({ supportFeatures: 'naviScreen,foo' })
    renderCluster()
    // supportsNaviScreen=true → the "Not supported by firmware" hint is hidden
    expect(screen.queryByText('Not supported by firmware')).not.toBeInTheDocument()
  })

  test('parseBoxInfo treats empty / invalid JSON strings as null', () => {
    liviState.boxInfo = '   '
    renderCluster()
    // No box → not supported → hint appears (isStreaming=true triggers it)
    expect(screen.getByText('Not supported by firmware')).toBeInTheDocument()
  })

  test('parseBoxInfo survives a non-JSON string', () => {
    liviState.boxInfo = 'this is not json'
    renderCluster()
    expect(screen.getByText('Not supported by firmware')).toBeInTheDocument()
  })

  test('supportFeatures array form matches naviScreen entry', () => {
    liviState.boxInfo = { supportFeatures: ['Foo', 'NaviScreen', 'Bar'] }
    renderCluster()
    expect(screen.queryByText('Not supported by firmware')).not.toBeInTheDocument()
  })

  test('isAaActive overrides missing firmware support', () => {
    liviState.boxInfo = null
    statusState.isAaActive = true
    renderCluster()
    expect(screen.queryByText('Not supported by firmware')).not.toBeInTheDocument()
    statusState.isAaActive = false
  })

  test('worker keyframe requests trigger requestCluster(true)', async () => {
    renderCluster()
    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-ready' })
    })
    ;(window as any).projection.ipc.requestCluster.mockClear()
    act(() => {
      worker.emit({ type: 'awaiting-keyframe' })
    })
    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(true)
    act(() => {
      worker.emit({ type: 'request-keyframe' })
    })
    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledTimes(2)
  })

  test('cluster-video-codec event posts SetCodecEvent to the worker', () => {
    const eventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      eventCbs.push(cb)
    })
    renderCluster()
    const worker = MockWorker.instances[0]
    worker.postMessage.mockClear()
    act(() => {
      eventCbs.forEach((cb) =>
        cb(undefined, { type: 'cluster-video-codec', payload: { codec: 'h265' } })
      )
    })
    const sent = worker.postMessage.mock.calls.find((c) => c[0]?.codec === 'h265')
    expect(sent).toBeTruthy()
  })

  test('cluster-video-codec event ignores unknown codecs', () => {
    const eventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      eventCbs.push(cb)
    })
    renderCluster()
    const worker = MockWorker.instances[0]
    worker.postMessage.mockClear()
    act(() => {
      eventCbs.forEach((cb) =>
        cb(undefined, { type: 'cluster-video-codec', payload: { codec: 'xyz' } })
      )
    })
    expect(worker.postMessage).not.toHaveBeenCalled()
  })

  test('onClusterResolution applies the crop math to the canvas', async () => {
    let resCb: ((p: unknown) => void) | null = null
    ;(window as any).projection.ipc.onClusterResolution = jest.fn((cb: (p: unknown) => void) => {
      resCb = cb
    })
    // Set user-facing cluster dims so clusterCrop branch fires
    liviState.settings = {
      fps: 60,
      clusterFps: 60,
      cluster: { main: true, dash: false, aux: false },
      clusterWidth: 800,
      clusterHeight: 480
    }
    const { container } = renderCluster()
    await waitFor(() => expect(resCb).not.toBeNull())
    act(() => {
      resCb!({ width: 1920, height: 1080 })
    })
    const canvas = container.querySelector('canvas') as HTMLCanvasElement
    // After resolution arrives, width style is computed (not literal 100%)
    expect(canvas.style.width.endsWith('%')).toBe(true)
  })

  test('onClusterResolution callback is skipped if IPC method is missing', () => {
    delete (window as any).projection.ipc.onClusterResolution
    expect(() => renderCluster()).not.toThrow()
  })

  test('plugged event re-requests cluster when ready', () => {
    const eventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      eventCbs.push(cb)
    })
    renderCluster()
    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-ready' })
    })
    ;(window as any).projection.ipc.requestCluster.mockClear()
    act(() => {
      eventCbs.forEach((cb) => cb(undefined, { type: 'plugged' }))
    })
    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(true)
  })

  test('plugged event is a no-op when cluster is not wanted', () => {
    liviState.settings = {
      fps: 60,
      clusterFps: 60,
      cluster: { main: false, dash: false, aux: false }
    }
    const eventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      eventCbs.push(cb)
    })
    renderCluster()
    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-ready' })
    })
    ;(window as any).projection.ipc.requestCluster.mockClear()
    act(() => {
      eventCbs.forEach((cb) => cb(undefined, { type: 'plugged' }))
    })
    expect((window as any).projection.ipc.requestCluster).not.toHaveBeenCalled()
  })

  test('becoming visible after being hidden requests the stream again', async () => {
    const { rerender } = render(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible={false} />
      </MemoryRouter>
    )
    const worker = MockWorker.instances[0]
    act(() => {
      worker.emit({ type: 'render-ready' })
    })
    ;(window as any).projection.ipc.requestCluster.mockClear()
    rerender(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible={true} />
      </MemoryRouter>
    )
    await waitFor(() => {
      expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(true)
    })
  })

  test('failure event resets codec and releases cluster', () => {
    const eventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = jest.fn((cb: AnyFn) => {
      eventCbs.push(cb)
    })
    renderCluster()
    const worker = MockWorker.instances[0]
    worker.postMessage.mockClear()
    ;(window as any).projection.ipc.requestCluster.mockClear()
    act(() => {
      eventCbs.forEach((cb) => cb(undefined, { type: 'failure' }))
    })
    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(false)
    // Both a SetCodecEvent (back to h264) and a reset are posted
    const types = worker.postMessage.mock.calls.map((c) => c[0]?.type)
    expect(types).toContain('setCodec')
    expect(types).toContain('reset')
    const setCodec = worker.postMessage.mock.calls.find((c) => c[0]?.type === 'setCodec')
    expect(setCodec?.[0]?.codec).toBe('h264')
  })
})
