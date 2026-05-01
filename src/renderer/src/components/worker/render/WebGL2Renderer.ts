import { FrameRenderer } from './Render.worker'

export class WebGL2Renderer implements FrameRenderer {
  #canvas: OffscreenCanvas | null = null
  #ctx: WebGL2RenderingContext | null = null
  #texture: WebGLTexture | null = null
  #w = 0
  #h = 0

  static vertexShaderSource = `
    attribute vec2 xy;
    varying highp vec2 uv;

    void main(void) {
      gl_Position = vec4(xy, 0.0, 1.0);
      uv = vec2((1.0 + xy.x) / 2.0, (1.0 - xy.y) / 2.0);
    }
  `

  static fragmentShaderSource = `
    varying highp vec2 uv;
    uniform sampler2D texture;

    void main(void) {
      gl_FragColor = texture2D(texture, uv);
    }
  `

  constructor(canvas: OffscreenCanvas) {
    this.#canvas = canvas
    const gl = (this.#ctx = canvas.getContext('webgl2'))
    if (!gl) throw Error('WebGL2 context is null')

    const vertexShader = gl.createShader(gl.VERTEX_SHADER)!
    gl.shaderSource(vertexShader, WebGL2Renderer.vertexShaderSource)
    gl.compileShader(vertexShader)

    const fragmentShader = gl.createShader(gl.FRAGMENT_SHADER)!
    gl.shaderSource(fragmentShader, WebGL2Renderer.fragmentShaderSource)
    gl.compileShader(fragmentShader)

    const shaderProgram = gl.createProgram()!
    gl.attachShader(shaderProgram, vertexShader)
    gl.attachShader(shaderProgram, fragmentShader)
    gl.linkProgram(shaderProgram)
    gl.useProgram(shaderProgram)

    const vertexBuffer = gl.createBuffer()!
    gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, -1, 1, 1, 1, 1, -1]), gl.STATIC_DRAW)

    const xyLocation = gl.getAttribLocation(shaderProgram, 'xy')
    gl.vertexAttribPointer(xyLocation, 2, gl.FLOAT, false, 0, 0)
    gl.enableVertexAttribArray(xyLocation)

    const texture = gl.createTexture()!
    gl.bindTexture(gl.TEXTURE_2D, texture)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE)
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE)

    this.#texture = texture
  }

  async draw(frame: VideoFrame): Promise<void> {
    const gl = this.#ctx!
    const bitmap = await createImageBitmap(frame)

    const w = frame.displayWidth
    const h = frame.displayHeight

    if (this.#canvas) {
      this.#canvas.width = w
      this.#canvas.height = h
    }

    if (w !== this.#w || h !== this.#h) {
      this.#w = w
      this.#h = h
      gl.bindTexture(gl.TEXTURE_2D, this.#texture!)
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null)
    }

    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight)
    gl.clearColor(0, 0, 0, 1)
    gl.clear(gl.COLOR_BUFFER_BIT)

    gl.bindTexture(gl.TEXTURE_2D, this.#texture!)
    gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, gl.RGBA, gl.UNSIGNED_BYTE, bitmap)

    gl.drawArrays(gl.TRIANGLE_FAN, 0, 4)

    bitmap.close()
    frame.close()
  }

  clear(): void {
    const gl = this.#ctx
    if (!gl) return
    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight)
    gl.clearColor(0, 0, 0, 1)
    gl.clear(gl.COLOR_BUFFER_BIT)
  }
}
