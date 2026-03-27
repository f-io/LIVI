import { WebGPURenderer } from '../WebGPURenderer'

describe('WebGPURenderer', () => {
  const configure = jest.fn()
  const getCurrentTexture = jest.fn()
  const createShaderModule = jest.fn((x) => x)
  const createRenderPipeline = jest.fn()
  const createSampler = jest.fn(() => ({ sampler: true }))
  const importExternalTexture = jest.fn()
  const createBindGroup = jest.fn(() => ({ bindGroup: true }))
  const createCommandEncoder = jest.fn()
  const requestDevice = jest.fn()
  const requestAdapter = jest.fn()
  const getPreferredCanvasFormat = jest.fn(() => 'bgra8unorm')
  const submit = jest.fn()
  const copyExternalImageToTexture = jest.fn()
  const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

  let device: any
  let ctx: any
  let adapter: any
  let commandEncoder: any
  let passEncoder: any
  let texture: any
  let pipeline: any
  let canvas: any

  beforeEach(() => {
    jest.clearAllMocks()

    texture = {
      createView: jest.fn(() => ({ textureView: true }))
    }

    passEncoder = {
      setPipeline: jest.fn(),
      setBindGroup: jest.fn(),
      draw: jest.fn(),
      end: jest.fn()
    }

    commandEncoder = {
      beginRenderPass: jest.fn(() => passEncoder),
      finish: jest.fn(() => ({ finished: true }))
    }

    pipeline = {
      getBindGroupLayout: jest.fn(() => ({ layout: true }))
    }

    device = {
      createShaderModule,
      createRenderPipeline: createRenderPipeline.mockReturnValue(pipeline),
      createSampler,
      importExternalTexture,
      createBindGroup,
      createCommandEncoder: createCommandEncoder.mockReturnValue(commandEncoder),
      queue: {
        submit,
        copyExternalImageToTexture
      }
    }

    adapter = {
      requestDevice: requestDevice.mockResolvedValue(device)
    }

    ctx = {
      configure,
      getCurrentTexture: getCurrentTexture.mockReturnValue(texture)
    }

    canvas = {
      width: 0,
      height: 0,
      getContext: jest.fn(() => ctx)
    }
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        gpu: {
          requestAdapter: requestAdapter.mockResolvedValue(adapter),
          getPreferredCanvasFormat
        }
      }
    })
  })

  afterAll(() => {
    warnSpy.mockRestore()
  })

  test('initializes webgpu pipeline in constructor and draws a frame', async () => {
    importExternalTexture.mockReturnValue({ externalTexture: true })

    const frame = {
      displayWidth: 320,
      displayHeight: 240,
      close: jest.fn()
    } as unknown as VideoFrame

    const renderer = new WebGPURenderer(canvas)
    await renderer.draw(frame)

    expect(requestAdapter).toHaveBeenCalledTimes(1)
    expect(requestDevice).toHaveBeenCalledTimes(1)
    expect(getPreferredCanvasFormat).toHaveBeenCalledTimes(1)
    expect(canvas.getContext).toHaveBeenCalledWith('webgpu')
    expect(configure).toHaveBeenCalledWith({
      device,
      format: 'bgra8unorm',
      alphaMode: 'opaque'
    })

    expect(canvas.width).toBe(320)
    expect(canvas.height).toBe(240)

    expect(importExternalTexture).toHaveBeenCalledWith({ source: frame })
    expect(createBindGroup).toHaveBeenCalledTimes(1)
    expect(commandEncoder.beginRenderPass).toHaveBeenCalledTimes(1)
    expect(passEncoder.setPipeline).toHaveBeenCalledWith(pipeline)
    expect(passEncoder.setBindGroup).toHaveBeenCalledTimes(1)
    expect(passEncoder.draw).toHaveBeenCalledWith(6, 1, 0, 0)
    expect(passEncoder.end).toHaveBeenCalledTimes(1)
    expect(submit).toHaveBeenCalledTimes(1)
    expect(frame.close).toHaveBeenCalledTimes(1)
  })

  test('falls back to copyExternalImageToTexture when importExternalTexture fails', async () => {
    const error = new Error('no external texture')
    importExternalTexture.mockImplementation(() => {
      throw error
    })

    const frame = {
      displayWidth: 640,
      displayHeight: 360,
      close: jest.fn()
    } as unknown as VideoFrame

    const renderer = new WebGPURenderer(canvas)
    await renderer.draw(frame)

    expect(copyExternalImageToTexture).toHaveBeenCalledWith(
      { source: frame },
      { texture },
      { width: 640, height: 360 }
    )
    expect(submit).not.toHaveBeenCalled()
    expect(frame.close).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalled()
  })

  test('throws when no WebGPU adapter is available', async () => {
    Object.defineProperty(global, 'navigator', {
      configurable: true,
      value: {
        gpu: {
          requestAdapter: jest.fn().mockResolvedValue(null),
          getPreferredCanvasFormat
        }
      }
    })

    const renderer = new WebGPURenderer(canvas)

    await expect(
      renderer.draw({
        displayWidth: 320,
        displayHeight: 240,
        close: jest.fn()
      } as unknown as VideoFrame)
    ).rejects.toThrow('WebGPU Adapter is null')
  })

  test('throws when webgpu context is not available', async () => {
    const badCanvas = {
      width: 0,
      height: 0,
      getContext: jest.fn(() => null)
    } as unknown as OffscreenCanvas

    const renderer = new WebGPURenderer(badCanvas)

    await expect(
      renderer.draw({
        displayWidth: 320,
        displayHeight: 240,
        close: jest.fn()
      } as unknown as VideoFrame)
    ).rejects.toThrow('Context is null')
  })
})
