const createComplexArrayMock = jest.fn()
const realTransformMock = jest.fn()
const completeSpectrumMock = jest.fn()

jest.mock('fft.js', () => {
  return jest.fn().mockImplementation(() => ({
    createComplexArray: createComplexArrayMock,
    realTransform: realTransformMock,
    completeSpectrum: completeSpectrumMock
  }))
})

describe('fft.worker', () => {
  let postedMessages: Array<{ message: any; transfer?: unknown }>
  let workerHandler: ((e: MessageEvent) => void) | undefined

  beforeEach(() => {
    jest.resetModules()
    postedMessages = []

    createComplexArrayMock.mockReset()
    realTransformMock.mockReset()
    completeSpectrumMock.mockReset()

    createComplexArrayMock.mockReturnValue(new Array(16).fill(0))
    ;(global as any).self = {
      postMessage: jest.fn((message: any, transfer?: unknown) => {
        postedMessages.push({ message, transfer })
      }),
      onmessage: undefined
    }

    require('../fft.worker')
    workerHandler = (global as any).self.onmessage
  })

  test('registers worker message handler', () => {
    expect(typeof workerHandler).toBe('function')
  })

  test('initializes fft worker state on init message', () => {
    workerHandler?.({
      data: {
        type: 'init',
        fftSize: 8,
        points: 4,
        sampleRate: 48000
      }
    } as MessageEvent)

    const FFT = require('fft.js')
    expect(FFT).toHaveBeenCalledWith(8)
    expect(createComplexArrayMock).toHaveBeenCalledTimes(1)
  })

  test('does nothing for pcm before init', () => {
    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.1, 0.2, 0.3, 0.4]).buffer
      }
    } as MessageEvent)

    expect(postedMessages).toHaveLength(0)
    expect(realTransformMock).not.toHaveBeenCalled()
  })

  test('does not emit bins when pcm buffer is shorter than fftSize', () => {
    workerHandler?.({
      data: {
        type: 'init',
        fftSize: 8,
        points: 4,
        sampleRate: 48000
      }
    } as MessageEvent)

    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.1, 0.2, 0.3, 0.4]).buffer
      }
    } as MessageEvent)

    expect(postedMessages).toHaveLength(0)
    expect(realTransformMock).not.toHaveBeenCalled()
  })

  test('processes one fft segment and posts normalized bins', () => {
    createComplexArrayMock.mockReturnValue(new Array(16).fill(0))

    realTransformMock.mockImplementation((output: number[]) => {
      for (let i = 0; i < output.length; i++) output[i] = 0

      // put some energy into a few bins
      output[2] = 20
      output[3] = 10
      output[4] = 16
      output[5] = 8
      output[6] = 12
      output[7] = 6
    })

    workerHandler?.({
      data: {
        type: 'init',
        fftSize: 8,
        points: 4,
        sampleRate: 48000
      }
    } as MessageEvent)

    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.2, 0.3, 0.4, 0.5, 0.4, 0.3, 0.2, 0.1]).buffer
      }
    } as MessageEvent)

    expect(realTransformMock).toHaveBeenCalledTimes(1)
    expect(completeSpectrumMock).toHaveBeenCalledTimes(1)
    expect(postedMessages).toHaveLength(1)

    const payload = postedMessages[0].message
    expect(payload.type).toBe('bins')
    expect(payload.bins).toBeInstanceOf(Float32Array)
    expect(payload.bins).toHaveLength(4)

    for (const value of payload.bins as Float32Array) {
      expect(value).toBeGreaterThanOrEqual(0)
      expect(value).toBeLessThanOrEqual(1)
    }
  })

  test('processes multiple fft segments from one pcm message', () => {
    createComplexArrayMock.mockReturnValue(new Array(16).fill(0))
    realTransformMock.mockImplementation((output: number[]) => {
      for (let i = 0; i < output.length; i++) output[i] = 0
      output[2] = 10
      output[3] = 5
    })

    workerHandler?.({
      data: {
        type: 'init',
        fftSize: 4,
        points: 3,
        sampleRate: 48000
      }
    } as MessageEvent)

    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8]).buffer
      }
    } as MessageEvent)

    expect(realTransformMock).toHaveBeenCalledTimes(2)
    expect(postedMessages).toHaveLength(2)
  })

  test('keeps leftover samples in ring buffer across pcm messages', () => {
    createComplexArrayMock.mockReturnValue(new Array(16).fill(0))
    realTransformMock.mockImplementation((output: number[]) => {
      for (let i = 0; i < output.length; i++) output[i] = 0
      output[2] = 10
      output[3] = 5
    })

    workerHandler?.({
      data: {
        type: 'init',
        fftSize: 8,
        points: 4,
        sampleRate: 48000
      }
    } as MessageEvent)

    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.1, 0.2, 0.3, 0.4]).buffer
      }
    } as MessageEvent)

    expect(postedMessages).toHaveLength(0)

    workerHandler?.({
      data: {
        type: 'pcm',
        buffer: new Float32Array([0.5, 0.6, 0.7, 0.8]).buffer
      }
    } as MessageEvent)

    expect(realTransformMock).toHaveBeenCalledTimes(1)
    expect(postedMessages).toHaveLength(1)
  })

  test('ignores unsupported message types', () => {
    workerHandler?.({
      data: {
        type: 'unknown'
      }
    } as MessageEvent)

    expect(postedMessages).toHaveLength(0)
    expect(realTransformMock).not.toHaveBeenCalled()
  })
})
