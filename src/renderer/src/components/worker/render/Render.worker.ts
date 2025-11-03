import { getDecoderConfig, getNaluFromStream, isKeyFrame, NaluTypes } from './lib/utils'
import { InitEvent, WorkerEvent } from './RenderEvents'
import { WebGL2Renderer } from './WebGL2Renderer'
import { WebGLRenderer } from './WebGLRenderer'
import { WebGPURenderer } from './WebGPURenderer'

export interface FrameRenderer {
  draw(data: VideoFrame): void
}

const scope = self as unknown as Worker

export class RendererWorker {
  private readonly vendorHeaderSize = 20
  private renderer: FrameRenderer | null = null
  private videoPort: MessagePort | null = null
  private pendingFrame: VideoFrame | null = null
  private startTime: number | null = null
  private frameCount = 0
  private fps = 0
  private decoder: VideoDecoder
  private isConfigured = false
  private lastSPS: Uint8Array | null = null
  private useHardware = false
  private awaitingValidKeyframe = true
  private hardwareAccelerationTested = false
  private selectedRenderer: string | null = null
  private renderScheduled = false
  private lastRenderTime: number = 0
  private frameInterval: number = 1000 / 60 // 60Hz

  constructor() {
    this.decoder = new VideoDecoder({
      output: this.onVideoDecoderOutput,
      error: this.onVideoDecoderOutputError
    })
  }

  private onVideoDecoderOutput = (frame: VideoFrame) => {
    if (this.startTime == null) {
      this.startTime = performance.now()
    } else {
      const elapsed = (performance.now() - this.startTime) / 1000
      this.fps = ++this.frameCount / elapsed
    }

    this.renderFrame(frame)
  }

  private renderFrame = (frame: VideoFrame) => {
    this.pendingFrame?.close()
    this.pendingFrame = frame

    if (!this.renderScheduled) {
      this.renderScheduled = true
      requestAnimationFrame(this.renderAnimationFrame)
    }
  }

  private renderAnimationFrame = () => {
    this.renderScheduled = false

    const now = performance.now()
    if (now - this.lastRenderTime < this.frameInterval * 0.75) {
      requestAnimationFrame(this.renderAnimationFrame)
      return
    }

    if (this.pendingFrame) {
      this.renderer?.draw(this.pendingFrame)
      this.pendingFrame = null
      this.lastRenderTime = now
    }
  }

  private onVideoDecoderOutputError = (err: Error) => {
    console.error(`[RENDER.WORKER] Decoder error`, err)
  }

  init = async (event: InitEvent & { platform?: string }) => {
    this.useHardware = event.useHardware

    this.videoPort = event.videoPort
    this.videoPort.onmessage = (ev: MessageEvent<ArrayBuffer>) => {
      this.processRaw(ev.data)
    }
    this.videoPort.start()

    self.postMessage({ type: 'render-ready' })
    console.debug('[RENDER.WORKER] render-ready')

    if (event.reportFps) {
      setInterval(() => {
        if (this.decoder.state === 'configured') {
          console.debug(`[RENDER.WORKER] FPS: ${this.fps.toFixed(2)}`)
        }
      }, 5000)
    }

    await this.evaluateRendererCapabilities()

    if (this.selectedRenderer === 'webgl2') {
      this.renderer = new WebGL2Renderer(event.canvas)
    } else if (this.selectedRenderer === 'webgl') {
      this.renderer = new WebGLRenderer(event.canvas)
    } else if (this.selectedRenderer === 'webgpu') {
      this.renderer = new WebGPURenderer(event.canvas)
    }

    if (!this.renderer) {
      console.warn('[RENDER.WORKER] No valid renderer selected, cannot proceed.')
    }
  }

  private async evaluateRendererCapabilities() {
    if (this.hardwareAccelerationTested) return

    console.debug('[RENDER.WORKER] Starting hardware acceleration tests...')

    const platform = navigator.platform.toLowerCase()
    const isMac = platform.startsWith('mac')
    const isLinux = platform.includes('linux')

    const rendererPriority =
      isMac
        ? ['webgpu', 'webgl2', 'webgl']
        : isLinux
          ? ['webgl2', 'webgl', 'webgpu']
          : ['webgl', 'webgl2', 'webgpu']

    const results: Record<string, { hw: boolean; sw: boolean; available: boolean }> = {}
    for (const r of rendererPriority) {
      results[r] = await this.isRendererSupported(r)
      console.debug(
        `[RENDER.WORKER] ${r.toUpperCase()}: available=${results[r].available}, ` +
        `hw=${results[r].hw}, sw=${results[r].sw}`
      )
    }

    const preferHW = this.useHardware ?? true
    const selectOrder: ('hw' | 'sw')[] = preferHW ? ['hw', 'sw'] : ['sw', 'hw']

    for (const mode of selectOrder) {
      for (const r of rendererPriority) {
        const caps = results[r]
        if (caps.available && caps[mode]) {
          this.selectedRenderer = r
          this.useHardware = mode === 'hw'
          this.hardwareAccelerationTested = true
          console.debug(
            `[RENDER.WORKER] Selected renderer: ${r} (` +
            `${mode === 'hw' ? 'hardware' : 'software'})`
          )
          return
        }
      }
    }

    console.warn('[RENDER.WORKER] No suitable renderer found')
  }

  private async isRendererSupported(
    renderer: string
  ): Promise<{ hw: boolean; sw: boolean; available: boolean }> {
    const canvas = new OffscreenCanvas(1, 1)
    let context: WebGLRenderingContext | WebGL2RenderingContext | GPUCanvasContext | null = null

    if (renderer === 'webgl2') {
      context = canvas.getContext('webgl2')
    } else if (renderer === 'webgl') {
      context = canvas.getContext('webgl')
    } else if (renderer === 'webgpu') {
      try {
        context = canvas.getContext('webgpu')
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
      } catch (e) {
        context = null
      }
    }

    if (!context) {
      return { hw: false, sw: false, available: false }
    }

    let hwSupported = false
    let swSupported = false

    const hwConfig: VideoDecoderConfig = {
      codec: 'avc1.64002A',
      hardwareAcceleration: 'prefer-hardware'
    }
    try {
      const hwSupportedResult = await VideoDecoder.isConfigSupported(hwConfig)
      hwSupported = !!hwSupportedResult.supported
    } catch (e) {
      console.warn(`[RENDER.WORKER] Error testing ${renderer} hardware:`, e)
    }

    const swConfig: VideoDecoderConfig = {
      codec: 'avc1.64002A',
      hardwareAcceleration: 'prefer-software'
    }
    try {
      const swSupportedResult = await VideoDecoder.isConfigSupported(swConfig)
      swSupported = !!swSupportedResult.supported
    } catch (e) {
      console.warn(`[RENDER.WORKER] Error testing ${renderer} software:`, e)
    }

    context = null

    return { hw: hwSupported, sw: swSupported, available: true }
  }

  private async configureDecoder(config: VideoDecoderConfig) {
    const accel = this.useHardware ? 'prefer-hardware' : 'prefer-software'
    const cfg: VideoDecoderConfig = {
      ...structuredClone(config),
      hardwareAcceleration: accel,
      optimizeForLatency: true
    }

    try {
      console.debug('[RENDER.WORKER] Configuring decoder with:', cfg)
      this.decoder.configure(cfg)
      this.isConfigured = true
      return true
    } catch (err) {
      console.warn(`[RENDER.WORKER] Config ${accel} error`, err)
      return false
    }
  }

  private async processRaw(buffer: ArrayBuffer) {
    if (!buffer.byteLength) return

    const data = new Uint8Array(buffer)
    const videoData =
      data.length > this.vendorHeaderSize ? data.subarray(this.vendorHeaderSize) : data

    const sps = getNaluFromStream(videoData, NaluTypes.SPS)
    const key = isKeyFrame(videoData)
    const now = performance.now()

    if (sps && !this.isConfigured) {
      console.debug('[RENDER.WORKER] SPS detected, length:', sps.rawNalu?.length)
      this.lastSPS = sps.rawNalu
    }

    if (this.awaitingValidKeyframe && !key) {
      console.debug('[RENDER.WORKER] Ignoring delta while awaiting keyframe...')
      return
    }

    if (key && this.lastSPS && !this.isConfigured) {
      console.debug('[RENDER.WORKER] First keyframe detected, attempting decoder config...')
      const config = getDecoderConfig(this.lastSPS)
      if (config && (await this.configureDecoder(config))) {
        try {
          const chunk = new EncodedVideoChunk({
            type: 'key',
            timestamp: now,
            data: videoData
          })
          this.decoder.decode(chunk)
          console.debug('[RENDER.WORKER] SPS+IDR sent')
          this.awaitingValidKeyframe = false
          return
        } catch (e) {
          console.warn('[RENDER.WORKER] Failed to decode first keyframe', e)
          return
        }
      }
    }

    if (!this.isConfigured || this.awaitingValidKeyframe) return

    const chunk = new EncodedVideoChunk({
      type: key ? 'key' : 'delta',
      timestamp: now,
      data: videoData
    })

    try {
      this.decoder.decode(chunk)
    } catch (e) {
      console.error('[RENDER.WORKER] Error during decoding:', e)
    }
  }
}

const worker = new RendererWorker()
scope.addEventListener('message', (event: MessageEvent<WorkerEvent>) => {
  if (event.data.type === 'init') {
    worker.init(event.data as InitEvent & { platform?: string })
  }
})

export { }
