import { render, screen } from '@testing-library/react'
import { TransportFlipIcon } from '../TransportFlipIcon'

jest.mock('@mui/icons-material/Sync', () => ({
  __esModule: true,
  default: (props: { sx?: { fontSize?: number } }) => (
    <svg data-testid="sync-icon" data-fontsize={props.sx?.fontSize} />
  )
}))

describe('TransportFlipIcon', () => {
  test('renders "D" overlay when the dongle transport is active', () => {
    const { container } = render(<TransportFlipIcon active="dongle" />)
    const letter = container.querySelector('span[aria-hidden="true"]')
    expect(letter?.textContent).toBe('D')
  })

  test('renders an empty overlay when AA is active', () => {
    const { container } = render(<TransportFlipIcon active="aa" />)
    const letter = container.querySelector('span[aria-hidden="true"]')
    expect(letter?.textContent).toBe('')
  })

  test('renders an empty overlay when nothing is active', () => {
    const { container } = render(<TransportFlipIcon active={null} />)
    const letter = container.querySelector('span[aria-hidden="true"]')
    expect(letter?.textContent).toBe('')
  })

  test('forwards a custom fontSize down to SyncIcon', () => {
    render(<TransportFlipIcon active="aa" fontSize={50} />)
    expect(screen.getByTestId('sync-icon')).toHaveAttribute('data-fontsize', '50')
  })
})
