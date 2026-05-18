import { render, screen } from '@testing-library/react'
import { TransportSwitchIcon } from '../TransportSwitchIcon'

jest.mock('@mui/icons-material/Sync', () => ({
  __esModule: true,
  default: (props: { sx?: { fontSize?: string | number } }) => (
    <svg data-testid="sync-icon" data-fontsize={String(props.sx?.fontSize ?? '')} />
  )
}))

jest.mock('@mui/icons-material/CableOutlined', () => ({
  __esModule: true,
  default: () => <svg data-testid="cable-icon" />
}))

jest.mock('@mui/icons-material/WifiOutlined', () => ({
  __esModule: true,
  default: () => <svg data-testid="wifi-icon" />
}))

jest.mock('@mui/icons-material/DeviceHub', () => ({
  __esModule: true,
  default: () => <svg data-testid="dongle-icon" />
}))

describe('TransportSwitchIcon', () => {
  test('renders a DeviceHub badge when the dongle transport is active', () => {
    render(<TransportSwitchIcon active="dongle" wiredPhoneActive={false} />)
    expect(screen.getByTestId('dongle-icon')).toBeInTheDocument()
  })

  test('renders a Cable badge for wired AA', () => {
    render(<TransportSwitchIcon active="aa" wiredPhoneActive={true} />)
    expect(screen.getByTestId('cable-icon')).toBeInTheDocument()
  })

  test('renders a WiFi badge for wireless AA', () => {
    render(<TransportSwitchIcon active="aa" wiredPhoneActive={false} />)
    expect(screen.getByTestId('wifi-icon')).toBeInTheDocument()
  })

  test('renders no badge when nothing is active', () => {
    const { container } = render(<TransportSwitchIcon active={null} wiredPhoneActive={false} />)
    expect(container.querySelector('span[aria-hidden="true"]')).toBeNull()
  })

  test('forwards a custom fontSize down to SyncIcon', () => {
    render(<TransportSwitchIcon active="aa" wiredPhoneActive={false} fontSize={50} />)
    expect(screen.getByTestId('sync-icon')).toHaveAttribute('data-fontsize', '50')
  })
})
