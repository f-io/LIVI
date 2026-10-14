import { render, waitFor } from '@testing-library/react'
import { Projection } from '../Projection'

const reportShownMock = vi.fn()
let mockPathname = '/'

type AnyFn = (...args: any[]) => any

const statusState: Record<string, any> = {
  isStreaming: true,
  activeProtocol: null
}

const liviState: Record<string, any> = {}

vi.mock('react-router', () => ({
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

  const reportShown = (screen: string, front: string) => reportShownMock(screen, front)

  return { reportShown, useStatusStore, useLiviStore, useProjectionActive }
})

vi.mock('../hooks/useProjectionTouch', () => ({
  useProjectionMultiTouch: () => ({})
}))

describe('Projection page', () => {
  beforeEach(() => {
    reportShownMock.mockReset()
    mockPathname = '/'

    statusState.isStreaming = true
    statusState.activeProtocol = null

    liviState.front = null
    liviState.sessions = { active: null }

    ;(global as any).ResizeObserver = vi.fn(function () {
      return {
        observe: vi.fn(),
        disconnect: vi.fn()
      }
    })
  })

  test('a phone that streams drives video visibility', () => {
    const setReceivingVideo = vi.fn()

    const { rerender } = render(<Projection {...baseProps({ setReceivingVideo })} />)
    expect(setReceivingVideo).toHaveBeenLastCalledWith(true)

    statusState.isStreaming = false
    rerender(<Projection {...baseProps({ setReceivingVideo })} />)
    expect(setReceivingVideo).toHaveBeenLastCalledWith(false)
  })

  test('the route tells core what the main screen shows', () => {
    const { rerender } = render(<Projection {...baseProps()} />)
    expect(reportShownMock).toHaveBeenLastCalledWith('main', 'projection')

    mockPathname = '/media'
    rerender(<Projection {...baseProps()} />)
    expect(reportShownMock).toHaveBeenLastCalledWith('main', 'livi')

    statusState.clusterDashboardActive = true
    rerender(<Projection {...baseProps()} />)
    expect(reportShownMock).toHaveBeenLastCalledWith('main', 'cluster')
    statusState.clusterDashboardActive = false
  })

  test('a connected phone turns the status overlay to its phone phase', () => {
    statusState.activeProtocol = 'carplay'

    const { container } = render(<Projection {...baseProps()} />)

    expect(container.querySelector('[role="status"]')).not.toBeNull()
  })

  test('the video shows through only on the projection route', () => {
    const { rerender } = render(<Projection {...baseProps({ receivingVideo: true })} />)
    expect(document.documentElement.classList.contains('show-video')).toBe(true)

    mockPathname = '/media'
    rerender(<Projection {...baseProps({ receivingVideo: true })} />)
    expect(document.documentElement.classList.contains('show-video')).toBe(false)
  })

  test('overlay offset recalc runs when content-root is in the DOM', async () => {
    const anchor = document.createElement('div')
    anchor.id = 'content-root'
    document.body.appendChild(anchor)

    // jsdom gives a zero DOMRect, recalc still runs through.
    expect(() => {
      render(<Projection {...baseProps()} />)
    }).not.toThrow()

    document.body.removeChild(anchor)
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

  test('touch and the view area mask sit in the stream box, not on the whole window', () => {
    const settings = baseSettings({
      projectionWidth: 1920,
      projectionHeight: 1080,
      projectionViewAreaTop: 54
    })
    const { container } = render(<Projection {...baseProps({ receivingVideo: true, settings })} />)

    const box = container.querySelector('#projection-stream') as HTMLElement
    expect(box.querySelector('#videoContainer')).not.toBeNull()
    expect(box.style.position).toBe('absolute')
    expect(box.style.left).toBe('50%')
    const mask = Array.from(box.children).filter((c) => c.id !== 'videoContainer')
    expect((mask[0] as HTMLElement).style.height).toBe('5%')
  })
})

function baseSettings(overrides: any = {}) {
  return {
    width: 800,
    height: 480,
    fps: 60,
    cluster: { main: false, cluster: false, aux: false },
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
      cluster: { main: false, cluster: false, aux: false }
    },
    ...overrides
  }
}
