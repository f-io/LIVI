import { render, screen } from '@testing-library/react'
import { Rpm } from '../Rpm'

const segmentDisplayMock = jest.fn()

jest.mock('../../components/SegmentDisplay', () => ({
  SegmentDisplay: (props: unknown) => {
    segmentDisplayMock(props)
    return <div data-testid="segment-display" />
  }
}))

describe('Rpm', () => {
  beforeEach(() => {
    segmentDisplayMock.mockClear()
  })

  test('renders default rpm 0 when no rpm is provided', () => {
    render(<Rpm />)

    expect(screen.getByText('RPM')).toBeInTheDocument()
    expect(screen.getByTestId('segment-display')).toBeInTheDocument()

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '0',
        digits: 4,
        offColor: 'transparent',
        offMode: 'blank'
      })
    )
  })

  test('rounds and passes through rpm within range', () => {
    render(<Rpm rpm={1234.6} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '1235',
        digits: 4
      })
    )
  })

  test('clamps rpm below 0 to 0', () => {
    render(<Rpm rpm={-10} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '0',
        digits: 4
      })
    )
  })

  test('clamps rpm above 9999 to 9999', () => {
    render(<Rpm rpm={12000} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '9999',
        digits: 4
      })
    )
  })

  test('applies className to root element', () => {
    const { container } = render(<Rpm rpm={2500} className="rpm-test" />)

    expect(container.firstChild).toHaveClass('rpm-test')
  })
})
