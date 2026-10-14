import { act, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router'
import { Cluster } from '../Cluster'

type AnyFn = (...args: any[]) => any

const statusState: Record<string, any> = { clusterDashboardActive: false }
const liviState: Record<string, any> = {}

vi.mock('../../../../store/store', async () => {
  const useStatusStore: any = (selector: AnyFn) => selector(statusState)
  const useLiviStore: any = (selector: AnyFn) => selector(liviState)
  return { useStatusStore, useLiviStore }
})

const renderCluster = (props: { visible?: boolean; showLoadingPlaceholder?: boolean } = {}) =>
  render(
    <MemoryRouter initialEntries={['/cluster']}>
      <Cluster {...props} />
    </MemoryRouter>
  )

describe('Cluster page', () => {
  beforeEach(() => {
    statusState.clusterDashboardActive = false
    liviState.settings = { clusterWidth: 800, clusterHeight: 480 }
    liviState.sessions = { active: null }
    ;(window as any).projection = { ipc: { clusterRepaintNudge: vi.fn().mockResolvedValue({}) } }
  })

  afterEach(() => {
    vi.useRealTimers()
    document.documentElement.classList.remove('show-cluster')
  })

  test('shows the map until a phone streams the cluster', () => {
    const { rerender } = renderCluster({ visible: true })
    expect(screen.getByTestId('MapOutlinedIcon')).toBeInTheDocument()

    liviState.sessions = { active: 'carplay' }
    rerender(
      <MemoryRouter initialEntries={['/cluster']}>
        <Cluster visible />
      </MemoryRouter>
    )
    expect(screen.queryByTestId('MapOutlinedIcon')).not.toBeInTheDocument()
  })

  test('shows no map while hidden or without the placeholder', () => {
    renderCluster()
    expect(screen.queryByTestId('MapOutlinedIcon')).not.toBeInTheDocument()
    renderCluster({ visible: true, showLoadingPlaceholder: false })
    expect(screen.queryByTestId('MapOutlinedIcon')).not.toBeInTheDocument()
  })

  test('lets the video show through only while visible', () => {
    const { unmount } = renderCluster({ visible: true })
    expect(document.documentElement.classList.contains('show-cluster')).toBe(true)
    unmount()
    expect(document.documentElement.classList.contains('show-cluster')).toBe(false)

    renderCluster()
    expect(document.documentElement.classList.contains('show-cluster')).toBe(false)
  })

  test('nudges the cluster repaint while the stream and the dashboard are on', () => {
    vi.useFakeTimers()
    statusState.clusterDashboardActive = true
    liviState.sessions = { active: 'carplay' }
    const nudge = vi.fn().mockRejectedValue(new Error('nudge'))
    ;(window as any).projection.ipc.clusterRepaintNudge = nudge

    renderCluster({ visible: true })
    act(() => {
      vi.advanceTimersByTime(200)
    })
    expect(nudge).toHaveBeenCalled()
  })

  test('does not throw when the repaint nudge is unavailable', () => {
    vi.useFakeTimers()
    statusState.clusterDashboardActive = true
    liviState.sessions = { active: 'carplay' }
    delete (window as any).projection.ipc.clusterRepaintNudge

    renderCluster({ visible: true })
    expect(() =>
      act(() => {
        vi.advanceTimersByTime(200)
      })
    ).not.toThrow()
  })

  test('falls back to no view area without settings', () => {
    liviState.settings = null
    expect(() => renderCluster({ visible: true })).not.toThrow()
  })
})
