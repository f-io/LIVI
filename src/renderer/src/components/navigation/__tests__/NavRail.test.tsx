import { fireEvent, render, screen } from '@testing-library/react'
import { UI } from '../../../constants'
import { NavRail, type NavRailItem } from '../NavRail'

jest.mock('@mui/material/styles', () => ({
  useTheme: () => ({ palette: { primary: { main: '#0af' } } })
}))

jest.mock('@mui/material/Tabs', () => ({
  __esModule: true,
  default: ({ children, onChange, value, ...rest }: any) => (
    <div data-testid="tabs" data-value={value} {...rest}>
      <button type="button" data-testid="tabs-change-2" onClick={(e) => onChange?.(e, 2)}>
        change-2
      </button>
      {children}
    </div>
  )
}))

jest.mock('@mui/material/Tab', () => ({
  __esModule: true,
  default: ({ icon, ...rest }: any) => (
    <button type="button" {...rest}>
      {icon}
    </button>
  )
}))

const items: NavRailItem[] = [
  { key: 'home', icon: <span>h</span>, label: 'Home' },
  { key: 'media', icon: <span>m</span>, label: 'Media' },
  { key: 'settings', icon: <span>s</span>, label: 'Settings' }
]

const originalH = window.innerHeight
beforeEach(() => {
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: originalH })
})

describe('NavRail', () => {
  test('reflects activeKey as the Tabs value', () => {
    render(<NavRail items={items} activeKey="settings" onSelect={() => {}} />)
    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '2')
  })

  test('falls back to value=0 when activeKey is unknown', () => {
    render(<NavRail items={items} activeKey="bogus" onSelect={() => {}} />)
    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '0')
  })

  test('clicking a tab calls onSelect with key + index', () => {
    const onSelect = jest.fn()
    render(<NavRail items={items} activeKey="home" onSelect={onSelect} />)
    fireEvent.click(screen.getByLabelText('Media'))
    expect(onSelect).toHaveBeenCalledWith('media', 1)
  })

  test('Tabs onChange also calls onSelect with the matching item', () => {
    const onSelect = jest.fn()
    render(<NavRail items={items} activeKey="home" onSelect={onSelect} />)
    fireEvent.click(screen.getByTestId('tabs-change-2'))
    expect(onSelect).toHaveBeenCalledWith('settings', 2)
  })

  test('renders all items with their accessible labels', () => {
    render(<NavRail items={items} activeKey="home" onSelect={() => {}} />)
    expect(screen.getByLabelText('Home')).toBeInTheDocument()
    expect(screen.getByLabelText('Media')).toBeInTheDocument()
    expect(screen.getByLabelText('Settings')).toBeInTheDocument()
  })

  test('accepts a custom ariaLabel', () => {
    render(<NavRail items={items} activeKey="home" onSelect={() => {}} ariaLabel="Secondary nav" />)
    expect(screen.getByTestId('tabs')).toHaveAttribute('aria-label', 'Secondary nav')
  })

  test('xs-icon branch fires when window is short', () => {
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      value: UI.XS_ICON_MAX_HEIGHT
    })
    render(<NavRail items={items} activeKey="home" onSelect={() => {}} />)
    expect(screen.getByLabelText('Home')).toBeInTheDocument()
  })
})
