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

  test('registers connection listener on socket.io server startup', () => {
    new TelemetrySocket(4100)

    expect(ioMock.on).toHaveBeenCalledWith(TelemetryEvents.Connection, expect.any(Function))
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

  test('publishTelemetry derives reverse from gear -1', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: -1 } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: -1 })
    )
    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, true)
  })

  test('publishTelemetry prefers explicit reverse over gear-derived value', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'R', reverse: false } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, false)
  })

  test('publishTelemetry does not emit reverse when it cannot derive a boolean', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'D' } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: 'D' })
    )
    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, false)
  })

  test('publishTelemetry does not emit lights when lights is not boolean', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'D', lights: 'yes' } as any)

    const lightsCalls = ioMock.emit.mock.calls.filter((c) => c[0] === TelemetryEvents.Lights)
    expect(lightsCalls).toHaveLength(0)
  })

  test('publishReverse updates cached last payload when one exists', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'D' } as any)
    ioMock.emit.mockClear()

    socket.publishReverse(true)

    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, true)

    socket.publishTelemetry({ gear: 'D' } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: 'D' })
    )
    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Reverse, false)
  })

  test('publishLights updates cached last payload when one exists', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishTelemetry({ gear: 'D' } as any)
    ioMock.emit.mockClear()

    socket.publishLights(true)

    expect(ioMock.emit).toHaveBeenCalledWith(TelemetryEvents.Lights, true)

    socket.publishTelemetry({ gear: 'D' } as any)

    expect(ioMock.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: 'D' })
    )
  })

  test('publishReverse still emits even when value did not change', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishReverse(false)
    socket.publishReverse(false)

    const reverseCalls = ioMock.emit.mock.calls.filter((c) => c[0] === TelemetryEvents.Reverse)
    expect(reverseCalls).toHaveLength(2)
  })

  test('publishLights still emits even when value did not change', () => {
    const socket = new TelemetrySocket(4100)
    ioMock.emit.mockClear()

    socket.publishLights(false)
    socket.publishLights(false)

    const lightsCalls = ioMock.emit.mock.calls.filter((c) => c[0] === TelemetryEvents.Lights)
    expect(lightsCalls).toHaveLength(2)
  })

  test('connection listener sends cached telemetry to new sockets', () => {
    const socket = new TelemetrySocket(4100)
    const connectionHandler = ioMock.on.mock.calls.find(
      ([event]) => event === TelemetryEvents.Connection
    )?.[1]

    socket.publishTelemetry({ gear: 'D', lights: true } as any)

    const clientSocket = {
      emit: jest.fn(),
      on: jest.fn()
    }

    connectionHandler(clientSocket)

    expect(clientSocket.emit).toHaveBeenCalledWith(
      TelemetryEvents.Update,
      expect.objectContaining({ gear: 'D', lights: true })
    )
    expect(clientSocket.on).toHaveBeenCalledWith(TelemetryEvents.Push, expect.any(Function))
  })

  test('connection listener forwards client telemetry push to event emitter and publishTelemetry', () => {
    const socket = new TelemetrySocket(4100)
    const publishSpy = jest.spyOn(socket, 'publishTelemetry')
    const pushListener = jest.fn()

    socket.on(TelemetryEvents.Push, pushListener)

    const connectionHandler = ioMock.on.mock.calls.find(
      ([event]) => event === TelemetryEvents.Connection
    )?.[1]

    const clientSocket = {
      emit: jest.fn(),
      on: jest.fn()
    }

    connectionHandler(clientSocket)

    const pushHandler = clientSocket.on.mock.calls.find(
      ([event]) => event === TelemetryEvents.Push
    )?.[1]

    const payload = { gear: 'R', lights: true } as any
    pushHandler(payload)

    expect(pushListener).toHaveBeenCalledWith(payload)
    expect(publishSpy).toHaveBeenCalledWith(payload)
  })

  test('disconnect closes io and http server', async () => {
    const socket = new TelemetrySocket(4100)

    await socket.disconnect()

    expect(ioMock.close).toHaveBeenCalled()
    expect(httpServerMock.close).toHaveBeenCalled()
    expect(socket.io).toBeNull()
    expect(socket.httpServer).toBeNull()
  })

  test('disconnect resolves when no http server exists', async () => {
    const socket = new TelemetrySocket(4100)
    socket.httpServer = null
    ioMock.close.mockClear()

    await expect(socket.disconnect()).resolves.toBeUndefined()

    expect(ioMock.close).toHaveBeenCalled()
    expect(socket.httpServer).toBeNull()
  })

  test('connect waits and starts server again', async () => {
    const socket = new TelemetrySocket(4100)
    const startServerSpy = jest.spyOn(socket as any, 'startServer')
    const timeoutSpy = jest.spyOn(global, 'setTimeout').mockImplementation(((
      fn: (...args: any[]) => void
    ) => {
      fn()
      return 0 as any
    }) as typeof setTimeout)

    await socket.connect()

    expect(timeoutSpy).toHaveBeenCalledWith(expect.any(Function), 200)
    expect(startServerSpy).toHaveBeenCalledTimes(1)
  })
})
