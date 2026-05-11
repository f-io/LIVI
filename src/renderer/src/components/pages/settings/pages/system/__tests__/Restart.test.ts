import { __resetRestartGuardForTests, Restart } from '../Restart'

describe('Restart', () => {
  beforeEach(() => {
    __resetRestartGuardForTests()
  })

  test('calls window.app.restartApp and returns null', () => {
    const catchMock = jest.fn()
    const restartAppMock = jest.fn(() => ({ catch: catchMock }))

    ;(window as any).app = {
      restartApp: restartAppMock
    }

    const result = Restart()

    expect(result).toBeNull()
    expect(restartAppMock).toHaveBeenCalledTimes(1)
    expect(catchMock).toHaveBeenCalledWith(console.error)
  })

  test('does not fire restartApp again on subsequent renders', () => {
    const catchMock = jest.fn()
    const restartAppMock = jest.fn(() => ({ catch: catchMock }))

    ;(window as any).app = {
      restartApp: restartAppMock
    }

    Restart()
    Restart()
    Restart()

    expect(restartAppMock).toHaveBeenCalledTimes(1)
  })
})
