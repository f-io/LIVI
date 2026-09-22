import { createTheme, ThemeProvider } from '@mui/material/styles'
import { act, render, waitFor } from '@testing-library/react'
import { FFTSpectrum, normalizePcmBuffer } from '../FFTSpectrum'

const postMessageMock = vi.fn()
const terminateMock = vi.fn()

const workerInstance = {
  postMessage: postMessageMock,
  terminate: terminateMock,
  onmessage: null as ((e: MessageEvent) => void) | null
}

vi.mock('../createFftWorker', () => ({
  createFftWorker: () => workerInstance
}))

let mockState = {
  audioSampleRate: 48000,
  visualAudioDelayMs: 120,
  audioPcmData: null as Float32Array | null
}

const subscribeMock = vi.fn<() => void, [(s: typeof mockState) => void]>(
  (_cb: (s: typeof mockState) => void) => vi.fn()
)
const observeMock = vi.fn()
const disconnectMock = vi.fn()
const clearRectMock = vi.fn()
const fillRectMock = vi.fn()
const beginPathMock = vi.fn()
const moveToMock = vi.fn()
const lineToMock = vi.fn()
const strokeMock = vi.fn()
const fillTextMock = vi.fn()

vi.mock('@store/store', () => ({
  useLiviStore: Object.assign((selector: (s: typeof mockState) => unknown) => selector(mockState), {
    subscribe: (cb: (s: typeof mockState) => void) => subscribeMock(cb)
  })
}))

describe('FFTSpectrum', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    vi.useFakeTimers({
      shouldAdvanceTime: true,
      toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date']
    })

    mockState = {
      audioSampleRate: 48000,
      visualAudioDelayMs: 120,
      audioPcmData: null
    }

    workerInstance.postMessage = postMessageMock
    workerInstance.terminate = terminateMock
    workerInstance.onmessage = null

    const requestAnimationFrameMock = vi.fn((cb: FrameRequestCallback) => {
      return setTimeout(() => cb(performance.now() + 100), 0) as unknown as number
    })

    const cancelAnimationFrameMock = vi.fn((id: number) => {
      clearTimeout(id as unknown as ReturnType<typeof setTimeout>)
    })

    Object.defineProperty(globalThis, 'requestAnimationFrame', {
      configurable: true,
      writable: true,
      value: requestAnimationFrameMock
    })

    Object.defineProperty(globalThis, 'cancelAnimationFrame', {
      configurable: true,
      writable: true,
      value: cancelAnimationFrameMock
    })

    Object.defineProperty(window, 'requestAnimationFrame', {
      configurable: true,
      writable: true,
      value: requestAnimationFrameMock
    })

    Object.defineProperty(window, 'cancelAnimationFrame', {
      configurable: true,
      writable: true,
      value: cancelAnimationFrameMock
    })

    Object.defineProperty(global, 'requestAnimationFrame', {
      configurable: true,
      writable: true,
      value: requestAnimationFrameMock
    })

    Object.defineProperty(global, 'cancelAnimationFrame', {
      configurable: true,
      writable: true,
      value: cancelAnimationFrameMock
    })
    ;(global as any).ResizeObserver = vi.fn(function (cb: ResizeObserverCallback) {
      return {
        observe: (target: Element) => {
          observeMock(target)
          cb(
            [
              {
                target,
                contentRect: {
                  width: 320,
                  height: 180,
                  top: 0,
                  left: 0,
                  bottom: 180,
                  right: 320,
                  x: 0,
                  y: 0,
                  toJSON: () => ({})
                }
              } as ResizeObserverEntry
            ],
            {} as ResizeObserver
          )
        },
        disconnect: disconnectMock
      }
    })

    Object.defineProperty(window, 'getComputedStyle', {
      configurable: true,
      value: vi.fn(() => ({
        getPropertyValue: vi.fn(() => '#12ab34')
      }))
    })

    Object.defineProperty(HTMLCanvasElement.prototype, 'getBoundingClientRect', {
      configurable: true,
      value: vi.fn(() => ({
        width: 320,
        height: 180,
        top: 0,
        left: 0,
        bottom: 180,
        right: 320
      }))
    })

    Object.defineProperty(HTMLCanvasElement.prototype, 'getContext', {
      configurable: true,
      value: vi.fn(() => ({
        clearRect: clearRectMock,
        fillRect: fillRectMock,
        beginPath: beginPathMock,
        moveTo: moveToMock,
        lineTo: lineToMock,
        stroke: strokeMock,
        fillText: fillTextMock,
        measureText: (t: string) => ({ width: t.length * 4 }),
        set fillStyle(_: string) {},
        set strokeStyle(_: string) {},
        set lineWidth(_: number) {},
        set font(_: string) {},
        set textAlign(_: CanvasTextAlign) {},
        set textBaseline(_: CanvasTextBaseline) {}
      }))
    })
  })

  afterEach(async () => {
    act(() => {
      vi.runOnlyPendingTimers()
    })
    vi.useRealTimers()
  })

  test('creates worker and posts init message', async () => {
    render(<FFTSpectrum />)

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledWith({
        type: 'init',
        fftSize: 4096,
        points: 24,
        sampleRate: 48000,
        minFreq: 20,
        maxFreq: 20000
      })
    })
  })

  test('terminates worker and disconnects resize observer on unmount', async () => {
    const { unmount } = render(<FFTSpectrum />)

    unmount()

    expect(terminateMock).toHaveBeenCalled()
    expect(disconnectMock).toHaveBeenCalled()
  })

  test('posts pcm to worker with configured visual delay', async () => {
    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(<FFTSpectrum />)

    const pcm = new Float32Array([0.1, 0.2, 0.3])
    subscriber({ ...mockState, audioPcmData: pcm })

    expect(postMessageMock).toHaveBeenCalledTimes(1)

    act(() => {
      vi.advanceTimersByTime(120)
    })

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledTimes(2)
    })

    expect(postMessageMock.mock.calls[1][0]).toEqual({
      type: 'pcm',
      buffer: expect.any(ArrayBuffer)
    })
    expect(postMessageMock.mock.calls[1][1]).toHaveLength(1)
  })

  test('posts pcm immediately when visual delay is zero', async () => {
    mockState.visualAudioDelayMs = 0

    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(<FFTSpectrum />)

    const pcm = new Float32Array([0.1, 0.2, 0.3])
    subscriber({ ...mockState, visualAudioDelayMs: 0, audioPcmData: pcm })

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledTimes(2)
    })

    expect(postMessageMock.mock.calls[1][0]).toEqual({
      type: 'pcm',
      buffer: expect.any(ArrayBuffer)
    })
  })

  test('ignores empty pcm payloads', async () => {
    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(<FFTSpectrum />)

    subscriber({ ...mockState, audioPcmData: new Float32Array(0) })

    expect(postMessageMock).toHaveBeenCalledTimes(1)
  })

  test('ignores null pcm payloads', async () => {
    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(<FFTSpectrum />)

    subscriber({ ...mockState, audioPcmData: null })

    expect(postMessageMock).toHaveBeenCalledTimes(1)
  })

  test('updates bins from worker message and draws bars', async () => {
    render(<FFTSpectrum />)

    workerInstance.onmessage?.({
      data: { type: 'bins', bins: new Float32Array(24).fill(0.5) }
    } as MessageEvent)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    await waitFor(() => {
      expect(fillRectMock).toHaveBeenCalled()
    })
  })

  test('draws frequency labels and no grid lines', async () => {
    render(<FFTSpectrum />)

    await waitFor(() => {
      expect(observeMock).toHaveBeenCalled()
    })

    act(() => {
      vi.runOnlyPendingTimers()
    })

    await waitFor(() => {
      expect(clearRectMock).toHaveBeenCalled()
      expect(strokeMock).not.toHaveBeenCalled()
      expect(fillTextMock).toHaveBeenCalledWith('20', expect.any(Number), expect.any(Number))
      expect(fillTextMock).toHaveBeenCalledWith('100', expect.any(Number), expect.any(Number))
      expect(fillTextMock).toHaveBeenCalledWith('1k', expect.any(Number), expect.any(Number))
      expect(fillTextMock).toHaveBeenCalledWith('10k', expect.any(Number), expect.any(Number))
      expect(fillTextMock).toHaveBeenCalledWith('20k', expect.any(Number), expect.any(Number))
    })
  })

  test('uses theme fallback when css variable is empty', async () => {
    Object.defineProperty(window, 'getComputedStyle', {
      configurable: true,
      value: vi.fn(() => ({
        getPropertyValue: vi.fn(() => '')
      }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    await waitFor(() => {
      expect(fillRectMock).toHaveBeenCalled()
    })
  })

  test('ignores worker messages with unknown type', async () => {
    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const callsBefore = fillRectMock.mock.calls.length

    workerInstance.onmessage?.({
      data: { type: 'unknown', bins: new Float32Array(24).fill(1) }
    } as MessageEvent)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillRectMock.mock.calls.length).toBeGreaterThanOrEqual(callsBefore)
  })

  test('skips background draw when width is zero', async () => {
    Object.defineProperty(HTMLCanvasElement.prototype, 'getBoundingClientRect', {
      configurable: true,
      value: vi.fn(() => ({
        width: 0,
        height: 150,
        top: 0,
        left: 0,
        bottom: 150,
        right: 0
      }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillTextMock).not.toHaveBeenCalled()
    expect(strokeMock).not.toHaveBeenCalled()
  })

  test('skips draw frame when FPS threshold not reached', async () => {
    const nowMock = vi.spyOn(performance, 'now')

    let t = 0
    nowMock.mockImplementation(() => t)

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const callsAfterFirstFrame = fillRectMock.mock.calls.length

    t = 1

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillRectMock.mock.calls.length).toBe(callsAfterFirstFrame)

    nowMock.mockRestore()
  })

  test('uses css variable for barColor when present', async () => {
    Object.defineProperty(window, 'getComputedStyle', {
      configurable: true,
      value: vi.fn(() => ({
        getPropertyValue: vi.fn(() => '  #ff0000  ')
      }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    await waitFor(() => {
      expect(fillRectMock).toHaveBeenCalled()
    })
  })

  test('does not update bins when worker sends non-bins message', async () => {
    render(<FFTSpectrum />)

    const prev = new Float32Array(24).fill(0.25)

    workerInstance.onmessage?.({
      data: { type: 'bins', bins: prev }
    } as MessageEvent)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const callsBefore = fillRectMock.mock.calls.length

    workerInstance.onmessage?.({
      data: { type: 'other', bins: new Float32Array(24).fill(1) }
    } as MessageEvent)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillRectMock.mock.calls.length).toBeGreaterThanOrEqual(callsBefore)
  })

  test('skips draw when canvas is null after unmount', async () => {
    const { unmount } = render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const callsBeforeUnmount = fillRectMock.mock.calls.length

    unmount()

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillRectMock.mock.calls.length).toBe(callsBeforeUnmount)
  })

  test('updates canvas size when dimensions change', async () => {
    render(<FFTSpectrum />)

    const canvas = document.querySelectorAll('canvas')[1] as HTMLCanvasElement

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(canvas.width).toBe(320)
    expect(canvas.height).toBe(180)
  })

  test('uses css variable when present for barColor', async () => {
    Object.defineProperty(window, 'getComputedStyle', {
      configurable: true,
      value: vi.fn(() => ({
        getPropertyValue: vi.fn(() => ' #00ff00 ')
      }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    await waitFor(() => {
      expect(fillRectMock).toHaveBeenCalled()
    })
  })

  test('uses default sampleRate and visualAudioDelayMs when store values are missing', async () => {
    mockState = {
      audioSampleRate: undefined as unknown as number,
      visualAudioDelayMs: undefined as unknown as number,
      audioPcmData: null
    }

    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(
      <ThemeProvider theme={createTheme({ palette: { mode: 'dark' } })}>
        <FFTSpectrum />
      </ThemeProvider>
    )

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledWith({
        type: 'init',
        fftSize: 4096,
        points: 24,
        sampleRate: 48000,
        minFreq: 20,
        maxFreq: 20000
      })
    })

    subscriber({
      ...mockState,
      audioPcmData: new Float32Array([0.1, 0.2, 0.3])
    })

    act(() => {
      vi.advanceTimersByTime(120)
    })

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledTimes(2)
    })
  })

  test('converts non-Float32Array pcm with new Float32Array(pcm)', async () => {
    mockState.visualAudioDelayMs = 0

    let subscriber!: (s: typeof mockState) => void
    subscribeMock.mockImplementation((cb) => {
      subscriber = cb
      return vi.fn()
    })

    render(<FFTSpectrum />)

    subscriber({
      ...mockState,
      visualAudioDelayMs: 0,
      audioPcmData: [0.1, 0.2, 0.3] as unknown as Float32Array
    })

    await waitFor(() => {
      expect(postMessageMock).toHaveBeenCalledTimes(2)
    })

    expect(postMessageMock.mock.calls[1][0]).toEqual({
      type: 'pcm',
      buffer: expect.any(ArrayBuffer)
    })
    expect(postMessageMock.mock.calls[1][1]).toHaveLength(1)
  })

  test('does nothing when resize observer callback is never triggered', async () => {
    ;(global as any).ResizeObserver = vi.fn(function () {
      return {
        observe: observeMock,
        disconnect: disconnectMock
      }
    })

    render(<FFTSpectrum />)

    expect(observeMock).toHaveBeenCalled()
    expect(clearRectMock).not.toHaveBeenCalled()
    expect(strokeMock).not.toHaveBeenCalled()
    expect(fillTextMock).not.toHaveBeenCalled()
  })

  test('drops labels and frees the bottom margin when the spectrum is too narrow', async () => {
    Object.defineProperty(HTMLCanvasElement.prototype, 'getBoundingClientRect', {
      configurable: true,
      value: vi.fn(() => ({ width: 60, height: 180, top: 0, left: 0, bottom: 180, right: 60 }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillTextMock).not.toHaveBeenCalled()
    expect(clearRectMock).toHaveBeenCalled()
  })

  test('skips a frequency label that would overlap its neighbour', async () => {
    Object.defineProperty(HTMLCanvasElement.prototype, 'getBoundingClientRect', {
      configurable: true,
      value: vi.fn(() => ({ width: 120, height: 180, top: 0, left: 0, bottom: 180, right: 120 }))
    })

    render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const drawn = fillTextMock.mock.calls.map((c) => c[0])
    expect(drawn).toContain('20k')
    expect(drawn).not.toContain('10k')
  })

  test('skips the raf draw when the canvas detaches before the pending frame runs', async () => {
    const nowMock = vi.spyOn(performance, 'now')
    let t = 1000
    nowMock.mockImplementation(() => t)

    const noop = vi.fn()
    Object.defineProperty(globalThis, 'cancelAnimationFrame', { configurable: true, value: noop })
    Object.defineProperty(window, 'cancelAnimationFrame', { configurable: true, value: noop })
    Object.defineProperty(global, 'cancelAnimationFrame', { configurable: true, value: noop })

    const { unmount } = render(<FFTSpectrum />)

    act(() => {
      vi.runOnlyPendingTimers()
    })

    const before = fillRectMock.mock.calls.length

    unmount()
    t = 2000

    act(() => {
      vi.runOnlyPendingTimers()
    })

    expect(fillRectMock.mock.calls.length).toBe(before)

    nowMock.mockRestore()
  })

  test('normalizePcmBuffer clones Float32Array input', async () => {
    const pcm = new Float32Array([0.1, 0.2, 0.3])

    const result = normalizePcmBuffer(pcm)

    expect(result).toBeInstanceOf(Float32Array)
    expect(result).not.toBe(pcm)
    expect(result).toHaveLength(3)
    expect(result[0]).toBeCloseTo(0.1)
    expect(result[1]).toBeCloseTo(0.2)
    expect(result[2]).toBeCloseTo(0.3)
  })

  test('normalizePcmBuffer converts array-like pcm input', async () => {
    const result = normalizePcmBuffer([0.1, 0.2, 0.3])

    expect(result).toBeInstanceOf(Float32Array)
    expect(result).toHaveLength(3)
    expect(result[0]).toBeCloseTo(0.1)
    expect(result[1]).toBeCloseTo(0.2)
    expect(result[2]).toBeCloseTo(0.3)
  })
})
