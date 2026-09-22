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

const statusState: Record<string, any> = {
  isStreaming: true
}
const liviState: Record<string, any> = {
  settings: { fps: 60, clusterFps: 60, cluster: { main: true, dash: false, aux: false } },
  boxInfo: null
}

vi.mock('../../../../store/store', async () => {
  const useStatusStore: any = (selector: AnyFn) => selector(statusState)
  const useLiviStore: any = (selector: AnyFn) => selector(liviState)
  useLiviStore.setState = (patch: Record<string, any>) => Object.assign(liviState, patch)
  return { useStatusStore, useLiviStore }
})

describe('Cluster page', () => {
  beforeAll(async () => {
    vi.spyOn(console, 'log').mockImplementation(() => {})
    vi.spyOn(console, 'warn').mockImplementation(() => {})
  })
  afterAll(async () => vi.restoreAllMocks())

  beforeEach(async () => {
    statusState.isStreaming = true
    liviState.settings = {
      fps: 60,
      clusterFps: 60,
      cluster: { main: true, dash: false, aux: false }
    }
    liviState.boxInfo = { supportFeatures: '' }
    ;(global as any).ResizeObserver = vi.fn(function () {
      return { observe: vi.fn(), disconnect: vi.fn() }
    })
    ;(global as any).MutationObserver = vi.fn(function () {
      return { observe: vi.fn(), disconnect: vi.fn() }
    })

    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    document.body.appendChild(contentRoot)
    ;(window as any).projection = {
      ipc: {
        requestCluster: vi.fn().mockResolvedValue(undefined),
        onClusterResolution: vi.fn(),
        onEvent: vi.fn(),
        offEvent: vi.fn()
      }
    }
  })

  test('releases cluster stream on phone disconnect', async () => {
    const projectionEventCbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = vi.fn((cb: AnyFn) => {
      projectionEventCbs.push(cb)
    })
    ;(window as any).projection.ipc.offEvent = vi.fn((cb: AnyFn) => {
      const i = projectionEventCbs.indexOf(cb)
      if (i >= 0) projectionEventCbs.splice(i, 1)
    })

    renderCluster()

    act(() => {
      projectionEventCbs.forEach((cb) => cb(undefined, { type: 'unplugged' }))
    })
    await waitFor(() => {
      expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(false)
    })
  })

  test('onClusterResolution hides the map placeholder once cluster frames arrive', async () => {
    let resCb: ((p: unknown) => void) | null = null
    ;(window as any).projection.ipc.onClusterResolution = vi.fn((cb: (p: unknown) => void) => {
      resCb = cb
    })
    liviState.boxInfo = { supportFeatures: 'naviScreen' }

    render(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible />
      </MemoryRouter>
    )

    await waitFor(() => expect(resCb).not.toBeNull())
    expect(screen.getAllByTestId('MapOutlinedIcon')).toHaveLength(1)

    act(() => {
      resCb!({ width: 1920, height: 1080 })
    })

    await waitFor(() => expect(screen.queryByTestId('MapOutlinedIcon')).not.toBeInTheDocument())
  })

  test('onClusterResolution callback is skipped if IPC method is missing', async () => {
    delete (window as any).projection.ipc.onClusterResolution
    expect(() => renderCluster()).not.toThrow()
  })

  test('requests the cluster again on a plugged event', async () => {
    const cbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = vi.fn((cb: AnyFn) => {
      cbs.push(cb)
      return vi.fn()
    })

    render(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible />
      </MemoryRouter>
    )
    ;(window as any).projection.ipc.requestCluster.mockClear()

    await act(async () => {
      cbs.forEach((cb) => cb(undefined, { type: 'plugged' }))
      await Promise.resolve()
    })

    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(true)
  })

  test('swallows requestCluster rejections on mount and on events', async () => {
    const cbs: AnyFn[] = []
    ;(window as any).projection.ipc.requestCluster = vi.fn().mockRejectedValue(new Error('boom'))
    ;(window as any).projection.ipc.onEvent = vi.fn((cb: AnyFn) => {
      cbs.push(cb)
      return vi.fn()
    })

    renderCluster()

    await waitFor(() => expect((window as any).projection.ipc.requestCluster).toHaveBeenCalled())

    await act(async () => {
      cbs.forEach((cb) => cb(undefined, { type: 'plugged' }))
      cbs.forEach((cb) => cb(undefined, { type: 'unplugged' }))
      await Promise.resolve()
    })

    expect((window as any).projection.ipc.requestCluster).toHaveBeenCalledWith(false)
  })

  test('handles projection events dispatched without a payload or unrelated types', async () => {
    const cbs: AnyFn[] = []
    ;(window as any).projection.ipc.onEvent = vi.fn((cb: AnyFn) => {
      cbs.push(cb)
      return vi.fn()
    })

    renderCluster()

    expect(() =>
      act(() => {
        cbs.forEach((cb) => cb(undefined))
        cbs.forEach((cb) => cb(undefined, { type: 'zzz' }))
        cbs.forEach((cb) => cb(undefined, { type: 'failure' }))
      })
    ).not.toThrow()
  })

  test('ignores zero-sized cluster frames and runs the resolution cleanup', async () => {
    let resCb: ((p: unknown) => void) | null = null
    const off = vi.fn()
    ;(window as any).projection.ipc.onClusterResolution = vi.fn((cb: (p: unknown) => void) => {
      resCb = cb
      return off
    })

    const { unmount } = renderCluster()

    await waitFor(() => expect(resCb).not.toBeNull())

    act(() => {
      resCb!(undefined)
      resCb!({ width: 'x', height: 'y' })
    })

    unmount()
    expect(off).toHaveBeenCalled()
  })

  test('nudges the cluster repaint when the stream and dash are active', async () => {
    vi.useFakeTimers()
    statusState.clusterDashActive = true
    let resCb: ((p: unknown) => void) | null = null
    const nudge = vi.fn().mockRejectedValue(new Error('nudge'))
    ;(window as any).projection.ipc.onClusterResolution = vi.fn((cb: (p: unknown) => void) => {
      resCb = cb
    })
    ;(window as any).projection.ipc.clusterRepaintNudge = nudge

    render(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible />
      </MemoryRouter>
    )

    act(() => {
      resCb!({ width: 100, height: 100 })
    })
    act(() => {
      vi.advanceTimersByTime(200)
    })

    expect(nudge).toHaveBeenCalled()

    vi.useRealTimers()
    statusState.clusterDashActive = false
  })

  test('does not throw when repaint nudge IPC is unavailable', async () => {
    vi.useFakeTimers()
    statusState.clusterDashActive = true
    let resCb: ((p: unknown) => void) | null = null
    ;(window as any).projection.ipc.onClusterResolution = vi.fn((cb: (p: unknown) => void) => {
      resCb = cb
    })
    delete (window as any).projection.ipc.clusterRepaintNudge

    render(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible />
      </MemoryRouter>
    )

    act(() => {
      resCb!({ width: 100, height: 100 })
    })

    expect(() =>
      act(() => {
        vi.advanceTimersByTime(200)
      })
    ).not.toThrow()

    vi.useRealTimers()
    statusState.clusterDashActive = false
  })
})
