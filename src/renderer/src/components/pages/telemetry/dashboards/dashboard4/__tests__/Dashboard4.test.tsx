import { render } from '@testing-library/react'

const setClusterDashboardActive = vi.fn()

vi.mock('@store/store', () => ({
  useStatusStore: (selector: (s: { setClusterDashboardActive: (v: boolean) => void }) => unknown) =>
    selector({ setClusterDashboardActive })
}))

import { Dashboard4 } from '../Dashboard4'

describe('Dashboard4', () => {
  beforeEach(() => {
    setClusterDashboardActive.mockClear()
  })

  test('marks the cluster dashboard active on mount and inactive on unmount', () => {
    const { unmount } = render(<Dashboard4 />)

    expect(setClusterDashboardActive).toHaveBeenCalledWith(true)

    setClusterDashboardActive.mockClear()
    unmount()

    expect(setClusterDashboardActive).toHaveBeenCalledWith(false)
  })

  test('renders an empty cluster-only box', () => {
    const { container } = render(<Dashboard4 />)

    expect(container.textContent).toBe('')
    expect(container.querySelector('div')).not.toBeNull()
  })
})
