import { render, screen } from '@testing-library/react'
import { Speed } from '../Speed'

const segmentDisplayMock = jest.fn()

jest.mock('../../components/SegmentDisplay', () => ({
  SegmentDisplay: (props: unknown) => {
    segmentDisplayMock(props)
    return <div data-testid="segment-display" />
  }
}))

describe('Speed', () => {
  beforeEach(() => {
    segmentDisplayMock.mockClear()
  })

  test('renders default speed 0 when no speed is provided', () => {
    render(<Speed />)

    expect(screen.getByText('KPH')).toBeInTheDocument()
    expect(screen.getByTestId('segment-display')).toBeInTheDocument()

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '0',
        digits: 3,
        offColor: 'transparent',
        offMode: 'blank'
      })
    )
  })

  test('rounds and passes through speed within range', () => {
    render(<Speed speedKph={87.6} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '88',
        digits: 3
      })
    )
  })

  test('clamps speed below 0 to 0', () => {
    render(<Speed speedKph={-5} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '0',
        digits: 3
      })
    )
  })

  test('clamps speed above 999 to 999', () => {
    render(<Speed speedKph={1200} />)

    expect(segmentDisplayMock).toHaveBeenCalledWith(
      expect.objectContaining({
        value: '999',
        digits: 3
      })
    )
  })

  test('applies className to root element', () => {
    const { container } = render(<Speed speedKph={80} className="speed-test" />)

    expect(container.firstChild).toHaveClass('speed-test')
  })
})
