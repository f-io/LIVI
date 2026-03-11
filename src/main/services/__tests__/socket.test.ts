const ioMock = {
  on: jest.fn(),
  emit: jest.fn(),
  close: jest.fn((cb?: () => void) => cb?.())
}

const httpServerMock = {
  listen: jest.fn((_port: number, cb?: () => void) => cb?.()),
  close: jest.fn((cb?: () => void) => cb?.())
}

jest.mock('socket.io', () => ({
  Server: jest.fn(() => ioMock)
}))

jest.mock('http', () => ({
  __esModule: true,
  default: {
    createServer: jest.fn(() => httpServerMock)
  }
}))

import { TelemetryEvents, TelemetrySocket } from '@main/services/Socket'

describe('TelemetrySocket', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('starts server and listens on provided port', () => {
    new TelemetrySocket(4100)
    expect(httpServerMock.listen).toHaveBeenCalledWith(4100, expect.any(Function))
  })

  test('publishTelemetry emits update and reverse/lights changes', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'R', lights: true } as any)
    socket.publishTelemetry({ gear: 'R', lights: true } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: 'R' })
    )
    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, true)
    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Lights, true)

    const reverseCalls = ioMock.emit.mock.calls.filter((c) => c[0] === TelemetryEvents.Reverse)
    const lightsCalls = ioMock.emit.mock.calls.filter((c) => c[0] === TelemetryEvents.Lights)
    expect(reverseCalls).toHaveLength(1)
    expect(lightsCalls).toHaveLength(1)
  })

  test('disconnect closes io and http server', async () => {
    const socket = new TelemetrySocket(4100)

    await socket.disconnect()

    expect(ioMock.close).toHaveBeenCalled()
    expect(httpServerMock.close).toHaveBeenCalled()
    expect(socket.io).toBeNull()
    expect(socket.httpServer).toBeNull()
  })
})
