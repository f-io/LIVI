import { render } from '@testing-library/react'

const setClusterDashActive = jest.fn()

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: { setClusterDashActive: (v: boolean) => void }) => unknown) =>
    selector({ setClusterDashActive })
}))

import { Dash4 } from '../Dash4'

describe('Dash4', () => {
  beforeEach(() => {
    setClusterDashActive.mockClear()
  })

  test('marks the cluster dash active on mount and inactive on unmount', () => {
    const { unmount } = render(<Dash4 />)

    expect(setClusterDashActive).toHaveBeenCalledWith(true)

    setClusterDashActive.mockClear()
    unmount()

    expect(setClusterDashActive).toHaveBeenCalledWith(false)
  })

  test('renders an empty cluster-only box', () => {
    const { container } = render(<Dash4 />)

    expect(container.textContent).toBe('')
    expect(container.querySelector('div')).not.toBeNull()
  })
})
