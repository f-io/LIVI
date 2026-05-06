export type WorkerEventType = 'init' | 'frame' | 'renderDone' | 'updateFps' | 'reset' | 'setCodec'

export type VideoCodec = 'h264' | 'h265' | 'vp9' | 'av1'

export interface WorkerEvent {
  type: WorkerEventType
}

export class RenderEvent implements WorkerEvent {
  type: WorkerEventType = 'frame'

  constructor(public frameData: ArrayBuffer) {}
}

export class InitEvent implements WorkerEvent {
  type: WorkerEventType = 'init'

  constructor(
    public canvas: OffscreenCanvas,
    public videoPort: MessagePort,
    public targetFps: number,
    public codec: VideoCodec = 'h264'
  ) {}
}

export class UpdateFpsEvent implements WorkerEvent {
  type: WorkerEventType = 'updateFps'

  constructor(public fps: number) {}
}

export class SetCodecEvent implements WorkerEvent {
  type: WorkerEventType = 'setCodec'

  constructor(public codec: VideoCodec) {}
}
