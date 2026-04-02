import { TelemetryEvents } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { getMainWindow } from '@main/window/createWindow'
import { EventEmitter } from 'events'

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn()
}))

describe('setupTelemetry', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('forwards telemetry push events to renderer channels', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({ webContents: { send } })

    const socket = new EventEmitter() as any
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(123)

    setupTelemetry(socket)

    socket.emit(TelemetryEvents.Push, { speed: 20, reverse: true, lights: false })

    expect(send).toHaveBeenCalledWith(TelemetryEvents.Update, {
      ts: 123,
      speed: 20,
      reverse: true,
      lights: false
    })
    expect(send).toHaveBeenCalledWith(TelemetryEvents.Reverse, true)
    expect(send).toHaveBeenCalledWith(TelemetryEvents.Lights, false)

    nowSpy.mockRestore()
  })

  test('does nothing when no main window exists', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    const socket = new EventEmitter() as any
    setupTelemetry(socket)

    expect(() => {
      socket.emit(TelemetryEvents.Push, { speed: 20, reverse: true, lights: false })
    }).not.toThrow()
  })

  test('only sends Update when reverse and lights are not booleans', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({ webContents: { send } })

    const socket = new EventEmitter() as any
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(456)

    setupTelemetry(socket)

    socket.emit(TelemetryEvents.Push, {
      speed: 30,
      reverse: 'yes',
      lights: 1
    })

    expect(send).toHaveBeenCalledTimes(1)
    expect(send).toHaveBeenCalledWith(TelemetryEvents.Update, {
      ts: 456,
      speed: 30,
      reverse: 'yes',
      lights: 1
    })

    nowSpy.mockRestore()
  })
})
