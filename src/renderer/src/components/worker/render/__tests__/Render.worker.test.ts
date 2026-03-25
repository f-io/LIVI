import { InitEvent } from '../RenderEvents'

const mockGetDecoderConfig = jest.fn()
const mockGetNaluFromStream = jest.fn()
const mockIsKeyFrame = jest.fn()

const mockWebGLDraw = jest.fn()
const mockWebGPUDraw = jest.fn()

const mockWebGL2Renderer = jest.fn().mockImplementation(() => ({ draw: mockWebGLDraw }))
const mockWebGPURenderer = jest.fn().mockImplementation(() => ({ draw: mockWebGPUDraw }))

jest.mock('../lib/utils', () => {
  const actual = jest.requireActual('../lib/utils')
  return {
    ...actual,
    getDecoderConfig: (...args: unknown[]) => mockGetDecoderConfig(...args),
    getNaluFromStream: (...args: unknown[]) => mockGetNaluFromStream(...args),
    isKeyFrame: (...args: unknown[]) => mockIsKeyFrame(...args)
  }
})

jest.mock('../WebGL2Renderer', () => ({ WebGL2Renderer: mockWebGL2Renderer }))
jest.mock('../WebGPURenderer', () => ({ WebGPURenderer: mockWebGPURenderer }))

describe('Render.worker', () => {
  let onMessageHandler: ((e: MessageEvent) => void) | null = null
  let postMessageSpy: jest.SpyInstance

  const decoderConfigure = jest.fn()
  const decoderDecode = jest.fn()

  beforeEach(() => {
    jest.clearAllMocks()
    jest.resetModules()
    onMessageHandler = null

    postMessageSpy = jest.spyOn(self, 'postMessage').mockImplementation(() => undefined)

    jest
      .spyOn(self, 'addEventListener')
      .mockImplementation((type: string, cb: EventListenerOrEventListenerObject) => {
        if (type === 'message') {
          onMessageHandler = cb as (e: MessageEvent) => void
        }
      })

    Object.defineProperty(global, 'performance', {
      configurable: true,
      value: { now: jest.fn(() => 1000) }
    })
    ;(global as any).requestAnimationFrame = jest.fn((cb: FrameRequestCallback) => {
      cb(1000)
      return 1
    })
    ;(global as any).OffscreenCanvas = class {
      constructor(
        public width: number,
        public height: number
      ) {}
      getContext(kind: string) {
        if (kind === 'webgl2') return { gl: true }
        if (kind === 'webgpu') return null
        return null
      }
    }
    ;(global as any).EncodedVideoChunk = class {
      type: string
      timestamp: number
      data: Uint8Array
      constructor(init: { type: string; timestamp: number; data: Uint8Array }) {
        this.type = init.type
        this.timestamp = init.timestamp
        this.data = init.data
      }
    }

    class MockVideoDecoder {
      static isConfigSupported = jest.fn(async () => ({ supported: true }))

      output?: (frame: any) => void
      error?: (err: Error) => void

      constructor(cfg: { output: (frame: any) => void; error: (err: Error) => void }) {
        this.output = cfg.output
        this.error = cfg.error
      }

      configure = decoderConfigure
      decode = decoderDecode
    }

    ;(global as any).VideoDecoder = MockVideoDecoder

    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        userAgent: 'linux x86_64',
        gpu: {
          requestAdapter: jest.fn(async () => ({ requestDevice: jest.fn(async () => ({})) }))
        }
      }
    })
  })

  afterEach(() => {
    postMessageSpy.mockRestore()
  })

  const importWorkerModule = async () => {
    return require('../Render.worker')
  }

  test('init selects renderer and posts render-ready', async () => {
    const { RendererWorker } = await importWorkerModule()

    const worker = new RendererWorker() as any
    const videoPort = {
      onmessage: null as any,
      start: jest.fn()
    } as unknown as MessagePort

    await worker.init(
      new InitEvent({ getContext: () => ({}) } as unknown as OffscreenCanvas, videoPort, 30)
    )

    expect(videoPort.start).toHaveBeenCalled()
    expect(mockWebGL2Renderer).toHaveBeenCalled()
    expect(postMessageSpy).toHaveBeenCalledWith({ type: 'render-ready' })
  })

  test('init posts render-error when no renderer context is available', async () => {
    ;(global as any).OffscreenCanvas = class {
      constructor(
        public width: number,
        public height: number
      ) {}
      getContext() {
        return null
      }
    }

    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    await worker.init(
      new InitEvent(
        { getContext: () => null } as unknown as OffscreenCanvas,
        { onmessage: null, start: jest.fn() } as unknown as MessagePort,
        60
      )
    )

    expect(postMessageSpy).toHaveBeenCalledWith(expect.objectContaining({ type: 'render-error' }))
  })

  test('processRaw configures decoder on first valid keyframe and decodes chunks', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.renderer = { draw: jest.fn() }
    worker.rendererHwSupported = true
    worker.rendererSwSupported = true

    mockGetNaluFromStream.mockReturnValue({ rawNalu: new Uint8Array([1, 2, 3, 4]) })
    mockGetDecoderConfig.mockReturnValue({
      codec: 'avc1.42001f',
      codedWidth: 800,
      codedHeight: 480
    })

    mockIsKeyFrame.mockReturnValueOnce(false)
    await worker.processRaw(new Uint8Array(40).buffer)
    expect(decoderDecode).not.toHaveBeenCalled()

    mockIsKeyFrame.mockReturnValue(true)
    await worker.processRaw(new Uint8Array(40).buffer)

    expect(decoderConfigure).toHaveBeenCalled()
    expect(decoderDecode).toHaveBeenCalled()
    expect(worker.awaitingValidKeyframe).toBe(false)
  })

  test('processRaw ignores empty buffers and missing renderer', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.renderer = null
    await worker.processRaw(new ArrayBuffer(0))
    await worker.processRaw(new ArrayBuffer(32))

    expect(decoderDecode).not.toHaveBeenCalled()
  })

  test('message listener routes init and updateFps events', async () => {
    await importWorkerModule()

    expect(onMessageHandler).toBeTruthy()

    const port = { onmessage: null, start: jest.fn() } as unknown as MessagePort
    onMessageHandler?.({
      data: {
        type: 'init',
        canvas: { getContext: () => ({}) },
        videoPort: port,
        targetFps: 24
      }
    } as MessageEvent)

    onMessageHandler?.({ data: { type: 'updateFps', fps: 20 } } as MessageEvent)

    expect(port.start).toHaveBeenCalled()
  })
  test('updateTargetFps ignores invalid and unchanged fps values', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.updateTargetFps(undefined)
    expect(worker.targetFps).toBeNull()

    worker.updateTargetFps(30)
    expect(worker.targetFps).toBe(30)
    expect(worker.frameInterval).toBeCloseTo(1000 / 30)

    worker.updateTargetFps(30)
    expect(worker.targetFps).toBe(30)
  })

  test('decoder output forwards frame into render pipeline', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    const close = jest.fn()
    const frame = { close } as unknown as VideoFrame
    const draw = jest.fn()

    worker.renderer = { draw }
    worker.frameInterval = 0

    worker.onVideoDecoderOutput(frame)

    expect(draw).toHaveBeenCalledWith(frame)
    expect(worker.pendingFrame).toBeNull()
  })

  test('renderFrame closes previous pending frame before replacing it', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    const oldFrame = { close: jest.fn() } as unknown as VideoFrame
    const newFrame = { close: jest.fn() } as unknown as VideoFrame

    worker.pendingFrame = oldFrame
    worker.renderer = { draw: jest.fn() }
    worker.frameInterval = 0

    worker.renderFrame(newFrame)

    expect((oldFrame as any).close).toHaveBeenCalled()
  })

  test('renderAnimationFrame reschedules when frame interval has not elapsed', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    const draw = jest.fn()
    worker.renderer = { draw }
    worker.pendingFrame = { close: jest.fn() } as unknown as VideoFrame
    worker.lastRenderTime = 990
    worker.frameInterval = 20
    ;(global as any).requestAnimationFrame = jest.fn()
    ;(global.performance.now as jest.Mock).mockReturnValue(1000)

    worker.renderAnimationFrame()

    expect(draw).not.toHaveBeenCalled()
    expect((global as any).requestAnimationFrame).toHaveBeenCalled()
  })

  test('onVideoDecoderOutputError logs decoder errors', async () => {
    const errorSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    const err = new Error('decode failed')
    worker.onVideoDecoderOutputError(err)

    expect(errorSpy).toHaveBeenCalled()

    errorSpy.mockRestore()
  })

  test('init posts render-error when renderer construction throws', async () => {
    mockWebGL2Renderer.mockImplementationOnce(() => {
      throw new Error('boom')
    })

    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    const videoPort = {
      onmessage: null as any,
      start: jest.fn()
    } as unknown as MessagePort

    await worker.init(
      new InitEvent({ getContext: () => ({}) } as unknown as OffscreenCanvas, videoPort, 30)
    )

    expect(postMessageSpy).toHaveBeenCalledWith(expect.objectContaining({ type: 'render-error' }))
  })

  test('configureDecoder falls back to software when hardware configure fails', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    let call = 0
    decoderConfigure.mockImplementation(() => {
      call += 1
      if (call === 1) {
        throw new Error('hw failed')
      }
    })

    worker.rendererHwSupported = true
    worker.rendererSwSupported = true

    const ok = await worker.configureDecoder({
      codec: 'avc1.42001f',
      codedWidth: 800,
      codedHeight: 480
    })

    expect(ok).toBe(true)
    expect(decoderConfigure).toHaveBeenCalledTimes(2)
  })

  test('configureDecoder posts render-error when neither hardware nor software works', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    decoderConfigure.mockImplementation(() => {
      throw new Error('fail')
    })

    worker.rendererHwSupported = true
    worker.rendererSwSupported = true

    const ok = await worker.configureDecoder({
      codec: 'avc1.42001f',
      codedWidth: 800,
      codedHeight: 480
    })

    expect(ok).toBe(false)
    expect(postMessageSpy).toHaveBeenCalledWith(expect.objectContaining({ type: 'render-error' }))
  })

  test('processRaw ignores delta frames while awaiting valid keyframe', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.renderer = { draw: jest.fn() }
    worker.awaitingValidKeyframe = true

    mockGetNaluFromStream.mockReturnValue(null)
    mockIsKeyFrame.mockReturnValue(false)

    await worker.processRaw(new Uint8Array(64).buffer)

    expect(decoderDecode).not.toHaveBeenCalled()
  })

  test('processRaw catches decode errors after configuration', async () => {
    const errorSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined)

    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.renderer = { draw: jest.fn() }
    worker.isConfigured = true
    worker.awaitingValidKeyframe = false

    mockGetNaluFromStream.mockReturnValue(null)
    mockIsKeyFrame.mockReturnValue(true)

    decoderDecode.mockImplementation(() => {
      throw new Error('decode crash')
    })

    await worker.processRaw(new Uint8Array(64).buffer)

    expect(errorSpy).toHaveBeenCalled()

    errorSpy.mockRestore()
  })
  test('evaluateRendererCapabilities returns early when already tested', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.hardwareAccelerationTested = true
    const spy = jest.spyOn(worker as any, 'isRendererSupported')

    await worker.evaluateRendererCapabilities()

    expect(spy).not.toHaveBeenCalled()
  })

  test('evaluateRendererCapabilities keeps selectedRenderer null when no renderer is available', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    jest.spyOn(worker as any, 'isRendererSupported').mockResolvedValue({
      hw: false,
      sw: false,
      available: false
    })

    await worker.evaluateRendererCapabilities()

    expect(worker.selectedRenderer).toBeNull()
    expect(worker.hardwareAccelerationTested).toBe(true)
  })

  test('configureDecoder skips hardware and uses software when hardware is not supported', async () => {
    decoderConfigure.mockReset()
    decoderConfigure.mockImplementation(() => undefined)
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    worker.rendererHwSupported = false
    worker.rendererSwSupported = true

    const ok = await worker.configureDecoder({
      codec: 'avc1.42001f',
      codedWidth: 800,
      codedHeight: 480
    })

    expect(ok).toBe(true)
    expect(decoderConfigure).toHaveBeenCalledTimes(1)
    expect(decoderConfigure).toHaveBeenCalledWith(
      expect.objectContaining({
        hardwareAcceleration: 'prefer-software'
      })
    )
  })

  test('message listener ignores unknown worker event types', async () => {
    await importWorkerModule()

    expect(onMessageHandler).toBeTruthy()

    onMessageHandler?.({
      data: { type: 'unknown-event' }
    } as MessageEvent)

    expect(postMessageSpy).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: 'render-error' })
    )
  })

  test('init posts render-error when no renderer instance is created after selection', async () => {
    const { RendererWorker } = await importWorkerModule()
    const worker = new RendererWorker() as any

    jest.spyOn(worker, 'evaluateRendererCapabilities').mockImplementation(async () => {
      worker.selectedRenderer = 'unknown-renderer'
      worker.hardwareAccelerationTested = true
    })

    const videoPort = {
      onmessage: null as any,
      start: jest.fn()
    } as unknown as MessagePort

    await worker.init(
      new InitEvent({ getContext: () => ({}) } as unknown as OffscreenCanvas, videoPort, 30)
    )

    expect(postMessageSpy).toHaveBeenCalledWith(expect.objectContaining({ type: 'render-error' }))
  })
})
