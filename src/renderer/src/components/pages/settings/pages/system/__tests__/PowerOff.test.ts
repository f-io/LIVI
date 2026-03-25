import { PowerOff } from '../PowerOff'

describe('PowerOff', () => {
  test('calls window.app.quitApp and returns null', () => {
    const catchMock = jest.fn()
    const quitAppMock = jest.fn(() => ({ catch: catchMock }))

    ;(window as any).app = {
      quitApp: quitAppMock
    }

    const result = PowerOff()

    expect(result).toBeNull()
    expect(quitAppMock).toHaveBeenCalledTimes(1)
    expect(catchMock).toHaveBeenCalledWith(console.error)
  })
})
