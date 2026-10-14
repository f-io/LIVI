import { renderHook } from '@testing-library/react'
import { useFollowFront } from '../useFollowFront'

const navigateMock = vi.fn()
let mockPathname = '/'
const liviState: Record<string, any> = {}

vi.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useLocation: () => ({ pathname: mockPathname })
}))

vi.mock('../../store/store', () => ({
  useLiviStore: (selector: (s: Record<string, any>) => unknown) => selector(liviState)
}))

/** The mocked router does not move, each step sets the page by hand. */
function follow(start: { path: string; front: string | null; phone?: boolean }) {
  mockPathname = start.path
  liviState.front = start.front ? { main: start.front } : null
  liviState.sessions = { active: start.phone === false ? null : 'carplay' }
  const hook = renderHook(() => useFollowFront())
  return {
    front(main: string) {
      liviState.front = { main }
      hook.rerender()
    },
    at(path: string) {
      mockPathname = path
      hook.rerender()
    }
  }
}

describe('useFollowFront', () => {
  beforeEach(() => {
    navigateMock.mockReset()
  })

  test('the projection core brings forward goes and the page before comes back', () => {
    const car = follow({ path: '/settings', front: 'livi' })
    car.front('projection')
    expect(navigateMock).toHaveBeenCalledWith('/', { replace: true })
    car.at('/')
    car.front('livi')
    expect(navigateMock).toHaveBeenLastCalledWith('/settings', { replace: true })
  })

  test('the phone handing the screen back leads to the media page', () => {
    const car = follow({ path: '/', front: 'projection' })
    car.front('livi')
    expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
  })

  test('a phone that is gone leaves the projection page to its idle look', () => {
    const car = follow({ path: '/', front: 'projection', phone: false })
    car.front('livi')
    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('the first state and the UI moving by itself do not navigate', () => {
    const car = follow({ path: '/media', front: null })
    car.front('projection')
    car.front('livi')
    car.at('/')
    car.front('projection')
    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('leaving for another page forgets the way back', () => {
    const car = follow({ path: '/settings', front: 'livi' })
    car.front('projection')
    for (const path of ['/', '/telemetry', '/']) car.at(path)
    navigateMock.mockClear()
    car.front('livi')
    expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
  })

  test('reversing shows the camera and gives the page back', () => {
    const car = follow({ path: '/media', front: 'livi' })
    car.front('camera')
    expect(navigateMock).toHaveBeenCalledWith('/camera', { replace: true })
    car.at('/camera')
    car.front('livi')
    expect(navigateMock).toHaveBeenLastCalledWith('/media', { replace: true })
  })

  test('a camera the user opened stays after reversing', () => {
    const car = follow({ path: '/camera', front: 'livi' })
    car.front('camera')
    car.front('livi')
    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('reversing during a call goes back to the call, then to the page before', () => {
    const car = follow({ path: '/media', front: 'livi' })
    car.front('projection')
    car.at('/')
    car.front('camera')
    car.at('/camera')
    car.front('projection')
    car.at('/')
    car.front('livi')
    expect(navigateMock.mock.calls.map(([path]) => path)).toEqual(['/', '/camera', '/', '/media'])
  })

  test('reversing from a projection the user opened ends on it and forgets it', () => {
    const car = follow({ path: '/', front: 'projection' })
    car.front('camera')
    car.at('/camera')
    car.front('projection')
    car.at('/')
    car.front('livi')
    expect(navigateMock.mock.calls.map(([path]) => path)).toEqual(['/camera', '/', '/media'])
  })
})
