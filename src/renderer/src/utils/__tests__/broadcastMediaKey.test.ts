import { broadcastMediaKey } from '../broadcastMediaKey'

describe('broadcastMediaKey', () => {
  test('dispatches car-media-key event with correct detail', () => {
    const dispatchSpy = jest.spyOn(window, 'dispatchEvent')

    broadcastMediaKey('play')

    expect(dispatchSpy).toHaveBeenCalledTimes(1)

    const event = dispatchSpy.mock.calls[0][0] as CustomEvent

    expect(event.type).toBe('car-media-key')
    expect(event.detail).toEqual({ command: 'play' })

    dispatchSpy.mockRestore()
  })
})
