import { fireEvent, render, screen } from '@testing-library/react'
import { DashboardsPagination } from '../pagination'

const usePaginationDotsMock = jest.fn()

jest.mock('@renderer/components/pages/telemetry/hooks/usePaginationDots', () => ({
  usePaginationDots: (isNavbarHidden: boolean) => usePaginationDotsMock(isNavbarHidden)
}))

describe('DashboardsPagination', () => {
  beforeEach(() => {
    jest.clearAllMocks()

    usePaginationDotsMock.mockReturnValue({
      showDots: true,
      revealDots: jest.fn()
    })
  })

  test('renders the correct number of dots', () => {
    render(
      <DashboardsPagination
        activeIndex={1}
        dotsLength={4}
        onSetIndex={jest.fn()}
        isNavbarHidden={false}
      />
    )

    expect(screen.getAllByRole('button')).toHaveLength(4)
  })

  test('calls usePaginationDots with isNavbarHidden', () => {
    render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={2}
        onSetIndex={jest.fn()}
        isNavbarHidden={true}
      />
    )

    expect(usePaginationDotsMock).toHaveBeenCalledWith(true)
  })

  test('calls onSetIndex and revealDots on pointer down', () => {
    const onSetIndex = jest.fn()
    const revealDots = jest.fn()

    usePaginationDotsMock.mockReturnValue({
      showDots: true,
      revealDots
    })

    render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={3}
        onSetIndex={onSetIndex}
        isNavbarHidden={false}
      />
    )

    fireEvent.pointerDown(screen.getAllByRole('button')[2])

    expect(onSetIndex).toHaveBeenCalledWith(2)
    expect(revealDots).toHaveBeenCalledTimes(1)
  })

  test('calls onSetIndex and revealDots on Enter key', () => {
    const onSetIndex = jest.fn()
    const revealDots = jest.fn()

    usePaginationDotsMock.mockReturnValue({
      showDots: true,
      revealDots
    })

    render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={3}
        onSetIndex={onSetIndex}
        isNavbarHidden={false}
      />
    )

    fireEvent.keyDown(screen.getAllByRole('button')[1], { key: 'Enter' })

    expect(onSetIndex).toHaveBeenCalledWith(1)
    expect(revealDots).toHaveBeenCalledTimes(1)
  })

  test('calls onSetIndex and revealDots on Space key', () => {
    const onSetIndex = jest.fn()
    const revealDots = jest.fn()

    usePaginationDotsMock.mockReturnValue({
      showDots: true,
      revealDots
    })

    render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={3}
        onSetIndex={onSetIndex}
        isNavbarHidden={false}
      />
    )

    fireEvent.keyDown(screen.getAllByRole('button')[1], { key: ' ' })

    expect(onSetIndex).toHaveBeenCalledWith(1)
    expect(revealDots).toHaveBeenCalledTimes(1)
  })

  test('does not react to other keys', () => {
    const onSetIndex = jest.fn()
    const revealDots = jest.fn()

    usePaginationDotsMock.mockReturnValue({
      showDots: true,
      revealDots
    })

    render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={3}
        onSetIndex={onSetIndex}
        isNavbarHidden={false}
      />
    )

    fireEvent.keyDown(screen.getAllByRole('button')[1], { key: 'Escape' })

    expect(onSetIndex).not.toHaveBeenCalled()
    expect(revealDots).not.toHaveBeenCalled()
  })

  test('hides pagination visually when showDots is false', () => {
    usePaginationDotsMock.mockReturnValue({
      showDots: false,
      revealDots: jest.fn()
    })

    const { container } = render(
      <DashboardsPagination
        activeIndex={0}
        dotsLength={2}
        onSetIndex={jest.fn()}
        isNavbarHidden={false}
      />
    )

    expect(container.firstChild).toHaveStyle({ opacity: '0' })
  })
})
