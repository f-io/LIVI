import { containsParameterSet, getDecoderConfig, getSpsFromStream, isKeyFrame } from './lib/utils'
import {
  InitEvent,
  SetCodecEvent,
  UpdateFpsEvent,
  type VideoCodec,
  WorkerEvent
} from './RenderEvents'
import { WebGL2Renderer } from './WebGL2Renderer'
import { WebGPURenderer } from './WebGPURenderer'

export interface FrameRenderer {
  draw(data: VideoFrame): void
  clear(): void
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
  private keyframeRetryTimer: ReturnType<typeof setInterval> | null = null
  private keyframeRetryCount = 0
  private pendingExtraData: Uint8Array | null = null
  private awaitingValidKeyframe = true
  private hardwareAccelerationTested = false
  private selectedRenderer: string | null = null
  private renderScheduled = false
  private lastRenderTime: number = 0
  private targetFps: number | null = null
  private frameInterval: number = 1000 / 60

  private rendererHwSupported = false
  private rendererSwSupported = false
  private hevcHwSupported = false
  private hevcSwSupported = false
  private vp9HwSupported = false
  private vp9SwSupported = false
  private av1HwSupported = false
  private av1SwSupported = false

  private codec: VideoCodec = 'h264'

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

  setCodec(codec: VideoCodec) {
    if (codec === this.codec) return
    console.log(`[RENDER.WORKER] codec change: ${this.codec} → ${codec}`)
    this.codec = codec
    this.resetDecoder()
  }

  private hasDecodedFrame = false

  private onVideoDecoderOutput = (frame: VideoFrame) => {
    if (!this.hasDecodedFrame) {
      this.hasDecodedFrame = true
      self.postMessage({ type: 'streaming' })
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

    this.resetDecoder()
  }

  private resetDecoder = () => {
    try {
      this.decoder.close()
    } catch {
      // already closed — fine
    }
    this.decoder = new VideoDecoder({
      output: this.onVideoDecoderOutput,
      error: this.onVideoDecoderOutputError
    })
    this.isConfigured = false
    this.awaitingValidKeyframe = true
    this.lastSPS = null
    this.pendingExtraData = null
    this.hasDecodedFrame = false
    // Drop the last decoded frame and blank the canvas so the user doesn't
    // see a frozen still from the previous session while we wait for the
    // phone to send a new SPS+IDR.
    if (this.pendingFrame) {
      try {
        this.pendingFrame.close()
      } catch {
        /* already closed */
      }
      this.pendingFrame = null
    }
    try {
      this.renderer?.clear()
    } catch {
      /* renderer not yet ready */
    }
    console.debug('[RENDER.WORKER] decoder reset — awaiting next SPS+IDR')
    // Tell the host UI we're not actually streaming — Projection.tsx flips
    // its `isStreaming` flag off so the streaming overlay shows again and
    // tab navigation works normally instead of trapping the user.
    self.postMessage({ type: 'awaiting-keyframe' })
    this.startKeyframeRetry()
  }

  private startKeyframeRetry = () => {
    this.stopKeyframeRetry()
    this.keyframeRetryCount = 0
    self.postMessage({ type: 'request-keyframe' })
    this.keyframeRetryCount += 1
    this.keyframeRetryTimer = setInterval(() => {
      if (!this.awaitingValidKeyframe || this.isConfigured) {
        this.stopKeyframeRetry()
        return
      }
      this.keyframeRetryCount += 1
      // Log only every 10th retry
      // phone-side stalls — once per ~20 s
      if (this.keyframeRetryCount % 5 === 0) {
        console.log(
          `[RENDER.WORKER] still waiting for IDR (codec=${this.codec}, retry #${this.keyframeRetryCount})`
        )
      }
      self.postMessage({ type: 'request-keyframe' })
    }, 2000)
  }

  private stopKeyframeRetry = () => {
    if (this.keyframeRetryTimer !== null) {
      clearInterval(this.keyframeRetryTimer)
      this.keyframeRetryTimer = null
    }
  }

  handleExternalReset(): void {
    this.stopKeyframeRetry()
    try {
      this.decoder.close()
    } catch {
      /* already closed */
    }
    this.decoder = new VideoDecoder({
      output: this.onVideoDecoderOutput,
      error: this.onVideoDecoderOutputError
    })
    this.isConfigured = false
    this.awaitingValidKeyframe = true
    this.lastSPS = null
    this.pendingExtraData = null
    this.hasDecodedFrame = false
    if (this.pendingFrame) {
      try {
        this.pendingFrame.close()
      } catch {
        /* already closed */
      }
      this.pendingFrame = null
    }
    try {
      this.renderer?.clear()
    } catch {
      /* renderer not ready */
    }
    console.debug('[RENDER.WORKER] external reset')
  }

  init = async (event: InitEvent) => {
    this.videoPort = event.videoPort
    this.videoPort.onmessage = (ev: MessageEvent<ArrayBuffer>) => {
      this.processRaw(ev.data)
    }
    this.videoPort.start()

    this.setTargetFps(event.targetFps)
    this.codec = event.codec ?? 'h264'
    console.log('[RENDER.WORKER] codec:', this.codec)

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

    const results: Record<
      string,
      {
        hw: boolean
        sw: boolean
        hevcHw: boolean
        hevcSw: boolean
        vp9Hw: boolean
        vp9Sw: boolean
        av1Hw: boolean
        av1Sw: boolean
        available: boolean
      }
    > = {}

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
        this.hevcHwSupported = caps.hevcHw
        this.hevcSwSupported = caps.hevcSw
        this.vp9HwSupported = caps.vp9Hw
        this.vp9SwSupported = caps.vp9Sw
        this.av1HwSupported = caps.av1Hw
        this.av1SwSupported = caps.av1Sw

        const mode = (hw: boolean, sw: boolean) => (hw ? 'hw' : sw ? 'sw' : 'unsupported')
        console.log(
          `[RENDER.WORKER] Selected renderer: ${r} ` +
            `(h264: ${mode(caps.hw, caps.sw)}, ` +
            `h265: ${mode(caps.hevcHw, caps.hevcSw)}, ` +
            `vp9: ${mode(caps.vp9Hw, caps.vp9Sw)}, ` +
            `av1: ${mode(caps.av1Hw, caps.av1Sw)})`
        )

        self.postMessage({
          type: 'codec-capabilities',
          capabilities: {
            h264: { hw: caps.hw, sw: caps.sw },
            h265: { hw: caps.hevcHw, sw: caps.hevcSw },
            vp9: { hw: caps.vp9Hw, sw: caps.vp9Sw },
            av1: { hw: caps.av1Hw, sw: caps.av1Sw }
          }
        })
        return
      }
    }

    this.hardwareAccelerationTested = true
    console.warn('[RENDER.WORKER] No suitable renderer found')
  }

  private async probeCodec(codec: string): Promise<{ hw: boolean; sw: boolean }> {
    let hw = false
    let sw = false
    try {
      const res = await VideoDecoder.isConfigSupported({
        codec,
        hardwareAcceleration: 'prefer-hardware'
      })
      hw = !!res.supported
      console.log(
        `[RENDER.WORKER] probe ${codec} prefer-hardware → supported=${!!res.supported}`,
        res.config ?? null
      )
    } catch (e) {
      console.warn(`[RENDER.WORKER] HW-probe error for ${codec}`, e)
    }
    try {
      const res = await VideoDecoder.isConfigSupported({
        codec,
        hardwareAcceleration: 'prefer-software'
      })
      sw = !!res.supported
      console.log(
        `[RENDER.WORKER] probe ${codec} prefer-software → supported=${!!res.supported}`,
        res.config ?? null
      )
    } catch (e) {
      console.warn(`[RENDER.WORKER] SW-probe error for ${codec}`, e)
    }
    // Cross-check with no-preference
    if (!hw && !sw) {
      try {
        const res = await VideoDecoder.isConfigSupported({ codec })
        console.log(
          `[RENDER.WORKER] probe ${codec} no-preference → supported=${!!res.supported}`,
          res.config ?? null
        )
      } catch (e) {
        console.warn(`[RENDER.WORKER] NoPref-probe error for ${codec}`, e)
      }
    }
    return { hw, sw }
  }

  private emptyRendererCaps(): {
    hw: boolean
    sw: boolean
    hevcHw: boolean
    hevcSw: boolean
    vp9Hw: boolean
    vp9Sw: boolean
    av1Hw: boolean
    av1Sw: boolean
    available: boolean
  } {
    return {
      hw: false,
      sw: false,
      hevcHw: false,
      hevcSw: false,
      vp9Hw: false,
      vp9Sw: false,
      av1Hw: false,
      av1Sw: false,
      available: false
    }
  }

  private async isRendererSupported(renderer: string): Promise<{
    hw: boolean
    sw: boolean
    hevcHw: boolean
    hevcSw: boolean
    vp9Hw: boolean
    vp9Sw: boolean
    av1Hw: boolean
    av1Sw: boolean
    available: boolean
  }> {
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
            return this.emptyRendererCaps()
          }
          await adapter.requestDevice()
        } catch (e) {
          console.debug('[RENDER.WORKER] WebGPU -> adapter/device init failed', e)
          return this.emptyRendererCaps()
        }
      }
    }

    if (!context) {
      console.debug(`[RENDER.WORKER] ${renderer.toUpperCase()} -> no context`)
      return this.emptyRendererCaps()
    }

    const h264 = await this.probeCodec('avc1.64002A')
    const h265 = await this.probeCodec('hvc1.1.6.L120.B0')
    const vp9 = await this.probeCodec('vp09.00.10.08')
    const av1 = await this.probeCodec('av01.0.04M.08')

    console.debug(
      `[RENDER.WORKER] ${renderer.toUpperCase()}: ` +
        `h264 hw=${h264.hw} sw=${h264.sw}, h265 hw=${h265.hw} sw=${h265.sw}, ` +
        `vp9 hw=${vp9.hw} sw=${vp9.sw}, av1 hw=${av1.hw} sw=${av1.sw}`
    )

    return {
      hw: h264.hw,
      sw: h264.sw,
      hevcHw: h265.hw,
      hevcSw: h265.sw,
      vp9Hw: vp9.hw,
      vp9Sw: vp9.sw,
      av1Hw: av1.hw,
      av1Sw: av1.sw,
      available: h264.hw || h264.sw
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
        console.log('[RENDER.WORKER] Configuring decoder with:', cfg)
        this.decoder.configure(cfg)
        this.isConfigured = true
        console.log(`[RENDER.WORKER] Selected decoder mode: ${hardwareAcceleration}`)
        return true
      } catch (err) {
        console.warn(`[RENDER.WORKER] Config ${hardwareAcceleration} error`, err)
        return false
      }
    }

    const hwSupp =
      this.codec === 'h265'
        ? this.hevcHwSupported
        : this.codec === 'vp9'
          ? this.vp9HwSupported
          : this.codec === 'av1'
            ? this.av1HwSupported
            : this.rendererHwSupported
    const swSupp =
      this.codec === 'h265'
        ? this.hevcSwSupported
        : this.codec === 'vp9'
          ? this.vp9SwSupported
          : this.codec === 'av1'
            ? this.av1SwSupported
            : this.rendererSwSupported

    if (hwSupp) {
      if (await tryConfig('prefer-hardware')) {
        return true
      }
    } else {
      console.debug(
        `[RENDER.WORKER] Skipping prefer-hardware (${this.codec}), not supported for selected renderer`
      )
    }

    if (swSupp) {
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

    const sps = getSpsFromStream(videoData, this.codec)
    const key = isKeyFrame(videoData, this.codec)
    const now = performance.now()

    // Detect mid-stream parameter-set change (resolution / profile / level).
    if (sps && this.isConfigured && this.lastSPS && !buffersEqual(sps.rawNalu, this.lastSPS)) {
      console.debug('[RENDER.WORKER] SPS changed mid-stream — resetting decoder')
      this.resetDecoder()
    }

    if (sps && !this.isConfigured) {
      console.log('[RENDER.WORKER] SPS detected, length:', sps.rawNalu?.length)
      this.lastSPS = sps.rawNalu
    }

    // Accumulate every param-set-bearing chunk before the first IDR
    if (!this.isConfigured && !key && containsParameterSet(videoData, this.codec)) {
      if (this.pendingExtraData) {
        const merged = new Uint8Array(this.pendingExtraData.length + videoData.length)
        merged.set(this.pendingExtraData, 0)
        merged.set(videoData, this.pendingExtraData.length)
        this.pendingExtraData = merged
      } else {
        this.pendingExtraData = videoData
      }
      console.log(
        `[RENDER.WORKER] Cached param-set chunk (${videoData.length}B, total ${this.pendingExtraData.length}B)`
      )
    }

    if (this.awaitingValidKeyframe && !key) {
      console.debug('[RENDER.WORKER] Ignoring delta while awaiting keyframe...')
      if (this.keyframeRetryTimer === null) this.startKeyframeRetry()
      return
    }

    if (key && this.lastSPS && !this.isConfigured) {
      console.log(`[RENDER.WORKER] First keyframe detected (${this.codec}), configuring decoder...`)
      const config = getDecoderConfig(this.lastSPS, this.codec)
      if (config && (await this.configureDecoder(config))) {
        try {
          // Annex-B mode: parameter sets must precede the IDR in the same chunk.
          // H.264 needs SPS+PPS, H.265 needs VPS+SPS+PPS. If the IDR frame
          // already contains the SPS (dongle path), use it as-is; otherwise
          // prepend the cached parameter-set chunks (AA path).
          let firstChunk = videoData
          const idrHasSps = getSpsFromStream(videoData, this.codec) !== null
          if (!idrHasSps && this.pendingExtraData) {
            firstChunk = new Uint8Array(this.pendingExtraData.length + videoData.length)
            firstChunk.set(this.pendingExtraData, 0)
            firstChunk.set(videoData, this.pendingExtraData.length)
            console.log(
              `[RENDER.WORKER] Prepended ${this.pendingExtraData.length}B param-sets to ${videoData.length}B IDR`
            )
          }
          const chunk = new EncodedVideoChunk({
            type: 'key',
            timestamp: now,
            data: firstChunk
          })
          this.decoder.decode(chunk)
          console.log('[RENDER.WORKER] keyframe sent to decoder')
          this.awaitingValidKeyframe = false
          this.pendingExtraData = null
          this.stopKeyframeRetry()
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
      this.resetDecoder()
    }
  }
}

function buffersEqual(a: Uint8Array | null, b: Uint8Array | null): boolean {
  if (!a || !b) return false
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false
  return true
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

    case 'setCodec':
      worker.setCodec((msg as SetCodecEvent).codec)
      break

    case 'reset':
      worker.handleExternalReset()
      break

    default:
      break
  }
})

export {}
