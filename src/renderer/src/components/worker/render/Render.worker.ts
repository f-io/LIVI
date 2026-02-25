import { getDecoderConfig, getNaluFromStream, isKeyFrame, NaluTypes } from './lib/utils'
import { InitEvent, WorkerEvent, UpdateFpsEvent } from './RenderEvents'
import { WebGL2Renderer } from './WebGL2Renderer'
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
  private decoder: VideoDecoder
  private isConfigured = false
  private lastSPS: Uint8Array | null = null
  private awaitingValidKeyframe = true
  private hardwareAccelerationTested = false
  private selectedRenderer: string | null = null
  private renderScheduled = false
  private lastRenderTime: number = 0
  private targetFps: number | null = null
  private frameInterval: number = 1000 / 60

  private rendererHwSupported = false
  private rendererSwSupported = false

  constructor() {
    this.decoder = new VideoDecoder({
      output: this.onVideoDecoderOutput,
      error: this.onVideoDecoderOutputError
    })
  }

  private setTargetFps(fps?: number) {
    if (!fps || !Number.isFinite(fps)) return

    const isFirst = this.targetFps == null
    if (!isFirst && fps === this.targetFps) return

    this.targetFps = fps
    this.frameInterval = 1000 / fps
    console.debug('[RENDER.WORKER] Using target FPS:', fps)
  }

  updateTargetFps(fps?: number) {
    this.setTargetFps(fps)
  }

  private onVideoDecoderOutput = (frame: VideoFrame) => {
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
    if (now - this.lastRenderTime < this.frameInterval) {
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

  init = async (event: InitEvent) => {
    this.videoPort = event.videoPort
    this.videoPort.onmessage = (ev: MessageEvent<ArrayBuffer>) => {
      this.processRaw(ev.data)
    }
    this.videoPort.start()

    this.setTargetFps(event.targetFps)

    await this.evaluateRendererCapabilities()

    if (!this.selectedRenderer) {
      console.warn('[RENDER.WORKER] No suitable renderer found')
      self.postMessage({ type: 'render-error', message: 'No renderer available' })
      return
    }

    try {
      if (this.selectedRenderer === 'webgl2') {
        this.renderer = new WebGL2Renderer(event.canvas)
      } else if (this.selectedRenderer === 'webgpu') {
        this.renderer = new WebGPURenderer(event.canvas)
      }
    } catch (e) {
      this.renderer = null
      console.warn('[RENDER.WORKER] Renderer init failed', e)
      self.postMessage({
        type: 'render-error',
        message: `Renderer init failed (${this.selectedRenderer})`
      })
      return
    }

    if (!this.renderer) {
      console.warn('[RENDER.WORKER] No valid renderer selected, cannot proceed.')
      self.postMessage({ type: 'render-error', message: 'No renderer available' })
      return
    }

    console.debug('[RENDER.WORKER] render-ready')
    self.postMessage({ type: 'render-ready' })
  }

  private async evaluateRendererCapabilities() {
    if (this.hardwareAccelerationTested) return

    console.debug('[RENDER.WORKER] Starting renderer capability tests...')

    const ua = navigator.userAgent.toLowerCase()
    const isMac = ua.includes('mac')
    const isLinux = ua.includes('linux')
    const isArm = ua.includes('aarch64') || ua.includes('arm64')

    const rendererPriority = isMac
      ? ['webgl2', 'webgpu'] // macOS -> WebGL2 first
      : isLinux && !isArm
        ? ['webgl2', 'webgpu'] // Linux x64 -> WebGL2 first
        : ['webgl2', 'webgpu'] // Linux ARM -> WebGL2 first

    const results: Record<string, { hw: boolean; sw: boolean; available: boolean }> = {}

    for (const r of rendererPriority) {
      results[r] = await this.isRendererSupported(r)
    }

    for (const r of rendererPriority) {
      const caps = results[r]
      if (caps.available) {
        this.selectedRenderer = r
        this.hardwareAccelerationTested = true
        this.rendererHwSupported = caps.hw
        this.rendererSwSupported = caps.sw
        console.debug(`[RENDER.WORKER] Selected renderer: ${r} (hw=${caps.hw}, sw=${caps.sw})`)
        return
      }
    }

    this.hardwareAccelerationTested = true
    console.warn('[RENDER.WORKER] No suitable renderer found')
  }

  private async isRendererSupported(
    renderer: string
  ): Promise<{ hw: boolean; sw: boolean; available: boolean }> {
    const canvas = new OffscreenCanvas(1, 1)
    let context: WebGL2RenderingContext | GPUCanvasContext | null = null

    if (renderer === 'webgl2') {
      context = canvas.getContext('webgl2')
    } else if (renderer === 'webgpu') {
      try {
        context = canvas.getContext('webgpu')
      } catch {
        context = null
      }

      if (context) {
        try {
          const adapter = await navigator.gpu?.requestAdapter()
          if (!adapter) {
            console.debug('[RENDER.WORKER] WebGPU -> adapter is null')
            return { hw: false, sw: false, available: false }
          }
          await adapter.requestDevice()
        } catch (e) {
          console.debug('[RENDER.WORKER] WebGPU -> adapter/device init failed', e)
          return { hw: false, sw: false, available: false }
        }
      }
    }

    if (!context) {
      console.debug(`[RENDER.WORKER] ${renderer.toUpperCase()} -> no context`)
      return { hw: false, sw: false, available: false }
    }

    let hwSupported = false
    let swSupported = false

    const hwConfig: VideoDecoderConfig = {
      codec: 'avc1.64002A',
      hardwareAcceleration: 'prefer-hardware'
    }

    try {
      const res = await VideoDecoder.isConfigSupported(hwConfig)
      hwSupported = !!res.supported
    } catch (e) {
      console.warn(`[RENDER.WORKER] ${renderer.toUpperCase()} HW-test error`, e)
    }

    const swConfig: VideoDecoderConfig = {
      codec: 'avc1.64002A',
      hardwareAcceleration: 'prefer-software'
    }

    try {
      const res = await VideoDecoder.isConfigSupported(swConfig)
      swSupported = !!res.supported
    } catch (e) {
      console.warn(`[RENDER.WORKER] ${renderer.toUpperCase()} SW-test error`, e)
    }

    console.debug(`[RENDER.WORKER] ${renderer.toUpperCase()}: hw=${hwSupported}, sw=${swSupported}`)

    return {
      hw: hwSupported,
      sw: swSupported,
      available: hwSupported || swSupported
    }
  }

  private async configureDecoder(config: VideoDecoderConfig) {
    const baseConfig: VideoDecoderConfig = {
      ...structuredClone(config),
      optimizeForLatency: true
    }

    const tryConfig = async (
      hardwareAcceleration: VideoDecoderConfig['hardwareAcceleration']
    ): Promise<boolean> => {
      const cfg: VideoDecoderConfig = { ...baseConfig, hardwareAcceleration }
      try {
        console.debug('[RENDER.WORKER] Configuring decoder with:', cfg)
        this.decoder.configure(cfg)
        this.isConfigured = true
        console.debug(`[RENDER.WORKER] Selected decoder mode: ${hardwareAcceleration}`)
        return true
      } catch (err) {
        console.warn(`[RENDER.WORKER] Config ${hardwareAcceleration} error`, err)
        return false
      }
    }

    if (this.rendererHwSupported) {
      if (await tryConfig('prefer-hardware')) {
        return true
      }
    } else {
      console.debug('[RENDER.WORKER] Skipping prefer-hardware, not supported for selected renderer')
    }

    if (this.rendererSwSupported) {
      if (await tryConfig('prefer-software')) {
        return true
      }
    }

    console.warn('[RENDER.WORKER] Failed to configure decoder (HW/SW not usable for renderer)')
    self.postMessage({
      type: 'render-error',
      message: 'Decoder not usable for selected renderer'
    })
    return false
  }

  private async processRaw(buffer: ArrayBuffer) {
    if (!buffer.byteLength) return
    if (!this.renderer) return

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
  const msg = event.data

  switch (msg.type) {
    case 'init':
      worker.init(msg as InitEvent)
      break

    case 'updateFps':
      worker.updateTargetFps((msg as UpdateFpsEvent).fps)
      break

    default:
      break
  }
})

export {}
