export type WorkerEventType = 'init' | 'frame' | 'renderDone' | 'updateFps' | 'reset'

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
    public targetFps: number
  ) {}
}

export class UpdateFpsEvent implements WorkerEvent {
  type: WorkerEventType = 'updateFps'

  constructor(public fps: number) {}
}
