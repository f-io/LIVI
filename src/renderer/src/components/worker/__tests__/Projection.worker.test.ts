describe('Projection.worker', () => {
  let selfMock: {
    onmessage: ((ev: MessageEvent<any>) => void) | null
    postMessage: jest.Mock
  }

  const originalSelf = global.self
  const originalConsoleError = console.error

  beforeEach(() => {
    jest.resetModules()

    selfMock = {
      onmessage: null,
      postMessage: jest.fn()
    }

    Object.defineProperty(global, 'self', {
      configurable: true,
      value: selfMock
    })

    console.error = jest.fn()
  })

  afterEach(() => {
    Object.defineProperty(global, 'self', {
      configurable: true,
      value: originalSelf
    })
    console.error = originalConsoleError
  })

  const loadWorker = () => {
    require('../Projection.worker')
  }

  test('sets up audio port on initialise and starts it', async () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    expect(typeof port.onmessage).toBe('function')
    expect(port.start).toHaveBeenCalledTimes(1)
  })

  test('processes Int16Array audio data from port and posts pcmData', async () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    const pcm = new Int16Array([32767, 0, -32768])

    port.onmessage?.({
      data: {
        type: 'audio',
        data: pcm,
        decodeType: 5
      }
    } as MessageEvent<any>)

    expect(selfMock.postMessage).toHaveBeenCalledTimes(1)

    const [message, transfer] = selfMock.postMessage.mock.calls[0]
    expect(message.type).toBe('pcmData')
    expect(message.decodeType).toBe(5)
    expect(message.payload).toBeInstanceOf(ArrayBuffer)
    expect(transfer).toHaveLength(1)

    const f32 = new Float32Array(message.payload)
    expect(f32[0]).toBeCloseTo(32767 / 32768)
    expect(f32[1]).toBe(0)
    expect(f32[2]).toBe(-1)
  })

  test('accepts ArrayBuffer via buffer property', async () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    const pcm = new Int16Array([1000, -1000])

    port.onmessage?.({
      data: {
        type: 'audio',
        buffer: pcm.buffer,
        decodeType: 3
      }
    } as MessageEvent<any>)

    expect(selfMock.postMessage).toHaveBeenCalledTimes(1)
    expect(selfMock.postMessage.mock.calls[0][0].decodeType).toBe(3)
  })

  test('accepts ArrayBuffer via chunk property', async () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    const pcm = new Int16Array([2000, -2000])

    port.onmessage?.({
      data: {
        type: 'audio',
        chunk: pcm.buffer,
        decodeType: 7
      }
    } as MessageEvent<any>)

    expect(selfMock.postMessage).toHaveBeenCalledTimes(1)
    expect(selfMock.postMessage.mock.calls[0][0].decodeType).toBe(7)
  })

  test('logs error when initialise payload has no audioPort', async () => {
    loadWorker()

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: {}
      }
    } as MessageEvent<any>)

    expect(console.error).toHaveBeenCalledWith(
      '[PROJECTION.WORKER] missing audioPort in initialise payload'
    )
  })

  test('logs error for unprocessable audio data', () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    port.onmessage?.({
      data: {
        type: 'audio',
        data: { not: 'pcm' },
        decodeType: 5
      }
    } as MessageEvent<any>)

    expect(console.error).toHaveBeenCalledWith(
      '[PROJECTION.WORKER] PCM - cannot interpret PCM data:',
      expect.objectContaining({
        type: 'audio',
        decodeType: 5,
        data: { not: 'pcm' }
      })
    )

    expect(selfMock.postMessage).not.toHaveBeenCalled()
  })

  test('posts failure when port setup throws', async () => {
    loadWorker()

    const port = {
      start: jest.fn(() => {
        throw new Error('boom')
      })
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    expect(console.error).toHaveBeenCalledWith(
      '[PROJECTION.WORKER] port setup failed:',
      expect.any(Error)
    )
    expect(selfMock.postMessage).toHaveBeenCalledWith({
      type: 'failure',
      error: 'Port setup failed'
    })
  })

  test('ignores stop and unknown commands', async () => {
    loadWorker()

    selfMock.onmessage?.({ data: { type: 'stop' } } as MessageEvent<any>)
    selfMock.onmessage?.({ data: { type: 'whatever' } } as MessageEvent<any>)

    expect(selfMock.postMessage).not.toHaveBeenCalled()
  })

  test('logs error when processing an audio message throws inside port handler', () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    const error = new Error('broken audio payload')
    const badAudioMessage = {
      type: 'audio',
      get buffer() {
        throw error
      }
    }

    port.onmessage?.({
      data: badAudioMessage
    } as MessageEvent<any>)

    expect(console.error).toHaveBeenCalledWith(
      '[PROJECTION.WORKER] error processing audio message:',
      error
    )
  })

  test('logs error when PCM payload is null', () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    port.onmessage?.({
      data: {
        type: 'audio',
        data: null,
        decodeType: 5
      }
    } as MessageEvent<any>)

    expect(selfMock.postMessage).not.toHaveBeenCalled()
  })

  test('ignores non-audio messages on the port even when data is present', () => {
    loadWorker()

    const port = {
      onmessage: null as ((ev: MessageEvent<any>) => void) | null,
      start: jest.fn()
    }

    selfMock.onmessage?.({
      data: {
        type: 'initialise',
        payload: { audioPort: port }
      }
    } as MessageEvent<any>)

    const pcm = new Int16Array([1, 2, 3])

    port.onmessage?.({
      data: {
        type: 'not-audio',
        data: pcm,
        decodeType: 5
      }
    } as MessageEvent<any>)

    expect(selfMock.postMessage).not.toHaveBeenCalled()
  })
})
