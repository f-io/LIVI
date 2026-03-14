import { fireEvent, render, screen } from '@testing-library/react'
import { SettingsLayout } from '../SettingsLayout'

const navigateMock = jest.fn()
let mockPathname = '/settings/system'

jest.mock('react-router', () => {
  const actual = jest.requireActual('react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useLocation: () => ({ pathname: mockPathname })
  }
})

describe('SettingsLayout', () => {
  beforeEach(() => {
    navigateMock.mockReset()
    mockPathname = '/settings/system'
    jest.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
      cb(0)
      return 1
    })
  })

  afterEach(() => {
    jest.restoreAllMocks()
  })

  test('shows Back button outside root settings page and navigates back on click', () => {
    render(
      <SettingsLayout title="System" showRestart={false}>
        <div>Body</div>
      </SettingsLayout>
    )

    const input = document.createElement('input')
    document.body.appendChild(input)
    input.focus()
    expect(document.activeElement).toBe(input)

    fireEvent.click(screen.getByLabelText('Back'))

    expect(navigateMock).toHaveBeenCalledWith(-1)
    expect(document.activeElement).not.toBe(input)
  })

  test('hides Back button on root settings page', () => {
    mockPathname = '/settings'
    render(
      <SettingsLayout title="Settings" showRestart={false}>
        <div>Body</div>
      </SettingsLayout>
    )

    expect(screen.queryByLabelText('Back')).toBeNull()
  })

  test('renders Apply action and calls restart handler', () => {
    const onRestart = jest.fn()
    render(
      <SettingsLayout title="System" showRestart onRestart={onRestart}>
        <div>Body</div>
      </SettingsLayout>
    )

    fireEvent.click(screen.getByLabelText('Apply'))
    expect(onRestart).toHaveBeenCalledTimes(1)
  })
})
