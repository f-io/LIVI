import { WebGL2Renderer } from '../WebGL2Renderer'

describe('WebGL2Renderer', () => {
  const createShader = jest.fn(() => ({}))
  const shaderSource = jest.fn()
  const compileShader = jest.fn()
  const createProgram = jest.fn(() => ({}))
  const attachShader = jest.fn()
  const linkProgram = jest.fn()
  const useProgram = jest.fn()
  const createBuffer = jest.fn(() => ({}))
  const bindBuffer = jest.fn()
  const bufferData = jest.fn()
  const getAttribLocation = jest.fn(() => 0)
  const vertexAttribPointer = jest.fn()
  const enableVertexAttribArray = jest.fn()
  const createTexture = jest.fn(() => ({}))
  const bindTexture = jest.fn()
  const texParameteri = jest.fn()
  const texImage2D = jest.fn()
  const viewport = jest.fn()
  const clearColor = jest.fn()
  const clear = jest.fn()
  const texSubImage2D = jest.fn()
  const drawArrays = jest.fn()

  const makeGl = () =>
    ({
      VERTEX_SHADER: 1,
      FRAGMENT_SHADER: 2,
      ARRAY_BUFFER: 3,
      STATIC_DRAW: 4,
      FLOAT: 5,
      TEXTURE_2D: 6,
      TEXTURE_MAG_FILTER: 7,
      TEXTURE_MIN_FILTER: 8,
      LINEAR: 9,
      TEXTURE_WRAP_S: 10,
      TEXTURE_WRAP_T: 11,
      CLAMP_TO_EDGE: 12,
      RGBA: 13,
      UNSIGNED_BYTE: 14,
      COLOR_BUFFER_BIT: 15,
      TRIANGLE_FAN: 16,
      drawingBufferWidth: 640,
      drawingBufferHeight: 360,

      createShader,
      shaderSource,
      compileShader,
      createProgram,
      attachShader,
      linkProgram,
      useProgram,
      createBuffer,
      bindBuffer,
      bufferData,
      getAttribLocation,
      vertexAttribPointer,
      enableVertexAttribArray,
      createTexture,
      bindTexture,
      texParameteri,
      texImage2D,
      viewport,
      clearColor,
      clear,
      texSubImage2D,
      drawArrays
    }) as unknown as WebGL2RenderingContext

  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('throws when webgl2 context is not available', () => {
    const canvas = {
      getContext: jest.fn(() => null)
    } as unknown as OffscreenCanvas

    expect(() => new WebGL2Renderer(canvas)).toThrow('WebGL2 context is null')
  })

  test('initializes shaders, buffers and texture in constructor', () => {
    const gl = makeGl()
    const canvas = {
      getContext: jest.fn(() => gl)
    } as unknown as OffscreenCanvas

    new WebGL2Renderer(canvas)

    expect(canvas.getContext).toHaveBeenCalledWith('webgl2')
    expect(createShader).toHaveBeenCalledTimes(2)
    expect(shaderSource).toHaveBeenCalledTimes(2)
    expect(compileShader).toHaveBeenCalledTimes(2)
    expect(createProgram).toHaveBeenCalledTimes(1)
    expect(attachShader).toHaveBeenCalledTimes(2)
    expect(linkProgram).toHaveBeenCalledTimes(1)
    expect(useProgram).toHaveBeenCalledTimes(1)
    expect(createBuffer).toHaveBeenCalledTimes(1)
    expect(bufferData).toHaveBeenCalledTimes(1)
    expect(createTexture).toHaveBeenCalledTimes(1)
    expect(texParameteri).toHaveBeenCalledTimes(4)
  })

  test('draw uploads texture storage on first size and renders frame', async () => {
    const gl = makeGl()
    const canvas = {
      width: 0,
      height: 0,
      getContext: jest.fn(() => gl)
    } as unknown as OffscreenCanvas

    const bitmap = {
      close: jest.fn()
    }

    const frame = {
      displayWidth: 320,
      displayHeight: 240,
      close: jest.fn()
    } as unknown as VideoFrame

    ;(global as any).createImageBitmap = jest.fn(async () => bitmap)

    const renderer = new WebGL2Renderer(canvas)
    await renderer.draw(frame)

    expect(canvas.width).toBe(320)
    expect(canvas.height).toBe(240)
    expect(texImage2D).toHaveBeenCalledTimes(1)
    expect(viewport).toHaveBeenCalledWith(0, 0, 640, 360)
    expect(clearColor).toHaveBeenCalledWith(0, 0, 0, 1)
    expect(clear).toHaveBeenCalledWith(15)
    expect(texSubImage2D).toHaveBeenCalledTimes(1)
    expect(drawArrays).toHaveBeenCalledWith(16, 0, 4)
    expect(bitmap.close).toHaveBeenCalledTimes(1)
    expect(frame.close).toHaveBeenCalledTimes(1)
  })

  test('does not recreate texture storage when size stays the same', async () => {
    const gl = makeGl()
    const canvas = {
      width: 0,
      height: 0,
      getContext: jest.fn(() => gl)
    } as unknown as OffscreenCanvas

    const frame1 = {
      displayWidth: 320,
      displayHeight: 240,
      close: jest.fn()
    } as unknown as VideoFrame

    const frame2 = {
      displayWidth: 320,
      displayHeight: 240,
      close: jest.fn()
    } as unknown as VideoFrame

    const bitmap1 = { close: jest.fn() }
    const bitmap2 = { close: jest.fn() }

    ;(global as any).createImageBitmap = jest
      .fn()
      .mockResolvedValueOnce(bitmap1)
      .mockResolvedValueOnce(bitmap2)

    const renderer = new WebGL2Renderer(canvas)

    await renderer.draw(frame1)
    await renderer.draw(frame2)

    expect(texImage2D).toHaveBeenCalledTimes(1)
    expect(texSubImage2D).toHaveBeenCalledTimes(2)
    expect(drawArrays).toHaveBeenCalledTimes(2)
    expect(bitmap1.close).toHaveBeenCalledTimes(1)
    expect(bitmap2.close).toHaveBeenCalledTimes(1)
    expect(frame1.close).toHaveBeenCalledTimes(1)
    expect(frame2.close).toHaveBeenCalledTimes(1)
  })
})
