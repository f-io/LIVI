import { broadcastMediaKey } from '../broadcastMediaKey'

describe('broadcastMediaKey', () => {
  const originalApp = (window as { app?: unknown }).app

  afterEach(() => {
    ;(window as { app?: unknown }).app = originalApp
  })

  test('forwards the command to main via the app preload bridge', () => {
    const broadcast = jest.fn()
    ;(window as unknown as { app: { broadcastMediaKey: typeof broadcast } }).app = {
      broadcastMediaKey: broadcast
    }
    const dispatchSpy = jest.spyOn(window, 'dispatchEvent')

    broadcastMediaKey('play')

    expect(broadcast).toHaveBeenCalledWith('play')
    expect(dispatchSpy).not.toHaveBeenCalled()

    dispatchSpy.mockRestore()
  })

  test('falls back to a local CustomEvent when the preload bridge is unavailable', () => {
    ;(window as { app?: unknown }).app = undefined
    const dispatchSpy = jest.spyOn(window, 'dispatchEvent')

    broadcastMediaKey('play')

    expect(dispatchSpy).toHaveBeenCalledTimes(1)
    const event = dispatchSpy.mock.calls[0][0] as CustomEvent
    expect(event.type).toBe('car-media-key')
    expect(event.detail).toEqual({ command: 'play' })

    dispatchSpy.mockRestore()
  })
})
