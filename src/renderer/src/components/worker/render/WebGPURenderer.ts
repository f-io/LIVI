import { FrameRenderer } from './Render.worker'

export class WebGPURenderer implements FrameRenderer {
  #canvas: OffscreenCanvas | null = null
  #ctx: GPUCanvasContext | null = null

  #started: Promise<void> | null = null

  #format: GPUTextureFormat | null = null
  #device: GPUDevice | null = null
  #pipeline: GPURenderPipeline | null = null
  #sampler: GPUSampler | null = null

  static vertexShaderSource = `
    struct VertexOutput {
      @builtin(position) Position: vec4<f32>,
      @location(0) uv: vec2<f32>,
    }

    @vertex
    fn vert_main(@builtin(vertex_index) VertexIndex: u32) -> VertexOutput {
      var pos = array<vec2<f32>, 6>(
        vec2<f32>( 1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(-1.0,  1.0)
      );

      var uv = array<vec2<f32>, 6>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 0.0)
      );

      var output : VertexOutput;
      output.Position = vec4<f32>(pos[VertexIndex], 0.0, 1.0);
      output.uv = uv[VertexIndex];
      return output;
    }
  `

  static fragmentShaderSource = `
    @group(0) @binding(1) var mySampler: sampler;
    @group(0) @binding(2) var myTexture: texture_external;

    @fragment
    fn frag_main(@location(0) uv : vec2<f32>) -> @location(0) vec4<f32> {
      return textureSampleBaseClampToEdge(myTexture, mySampler, uv);
    }
  `

  constructor(canvas: OffscreenCanvas) {
    this.#canvas = canvas
    this.#started = this.#start()
  }

  async #start() {
    const adapter = await navigator.gpu.requestAdapter()
    if (!adapter) throw new Error('WebGPU Adapter is null')

    this.#device = await adapter.requestDevice()
    this.#format = navigator.gpu.getPreferredCanvasFormat()

    if (!this.#canvas) throw new Error('Canvas is null')
    this.#ctx = this.#canvas.getContext('webgpu')
    if (!this.#ctx) throw new Error('Context is null')

    this.#ctx.configure({
      device: this.#device,
      format: this.#format,
      alphaMode: 'opaque'
    })

    this.#pipeline = this.#device.createRenderPipeline({
      layout: 'auto',
      vertex: {
        module: this.#device.createShaderModule({
          code: WebGPURenderer.vertexShaderSource
        }),
        entryPoint: 'vert_main'
      },
      fragment: {
        module: this.#device.createShaderModule({
          code: WebGPURenderer.fragmentShaderSource
        }),
        entryPoint: 'frag_main',
        targets: [{ format: this.#format! }]
      },
      primitive: {
        topology: 'triangle-list'
      }
    })

    this.#sampler = this.#device.createSampler()
  }

  async draw(frame: VideoFrame): Promise<void> {
    await this.#started
    if (!this.#canvas || !this.#ctx || !this.#device || !this.#pipeline || !this.#sampler) return

    // 1) Canvas auf sichtbare Abmessungen setzen
    this.#canvas.width = frame.displayWidth
    this.#canvas.height = frame.displayHeight

    // 2) Versuch, importExternalTexture zu verwenden
    let externalTexture: GPUExternalTexture
    try {
      externalTexture = this.#device.importExternalTexture({ source: frame })
    } catch (e) {
      console.warn(
        '[WebGPURenderer] importExternalTexture failed, falling back to copyExternalImageToTexture:',
        e
      )
      
      ;(this.#device.queue as any).copyExternalImageToTexture(
        { source: frame },
        { texture: this.#ctx.getCurrentTexture() },
        [frame.displayWidth, frame.displayHeight]
      )
      frame.close()
      return
    }

    const uniformBindGroup = this.#device.createBindGroup({
      layout: this.#pipeline.getBindGroupLayout(0),
      entries: [
        { binding: 1, resource: this.#sampler },
        { binding: 2, resource: externalTexture }
      ]
    })

    const commandEncoder = this.#device.createCommandEncoder()
    const textureView = this.#ctx.getCurrentTexture().createView()

    const renderPassDescriptor: GPURenderPassDescriptor = {
      colorAttachments: [
        {
          view: textureView,
          clearValue: [0.0, 0.0, 0.0, 1.0],
          loadOp: 'clear',
          storeOp: 'store'
        }
      ]
    }

    const passEncoder = commandEncoder.beginRenderPass(renderPassDescriptor)
    passEncoder.setPipeline(this.#pipeline)
    passEncoder.setBindGroup(0, uniformBindGroup)
    passEncoder.draw(6, 1, 0, 0)
    passEncoder.end()

    this.#device.queue.submit([commandEncoder.finish()])
    frame.close()
  }
}
