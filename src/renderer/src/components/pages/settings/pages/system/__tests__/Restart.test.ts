import { Restart } from '../Restart'

describe('Restart', () => {
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
})
