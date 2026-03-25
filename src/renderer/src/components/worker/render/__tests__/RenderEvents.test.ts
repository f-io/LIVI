import { InitEvent, RenderEvent, UpdateFpsEvent } from '../RenderEvents'

describe('RenderEvents', () => {
  test('RenderEvent creates a frame event with frameData payload', () => {
    const frameData = new ArrayBuffer(16)

    const event = new RenderEvent(frameData)

    expect(event.type).toBe('frame')
    expect(event.frameData).toBe(frameData)
  })

  test('InitEvent creates an init event with canvas, videoPort and targetFps', () => {
    const canvas = {} as OffscreenCanvas
    const videoPort = {} as MessagePort

    const event = new InitEvent(canvas, videoPort, 30)

    expect(event.type).toBe('init')
    expect(event.canvas).toBe(canvas)
    expect(event.videoPort).toBe(videoPort)
    expect(event.targetFps).toBe(30)
  })

  test('UpdateFpsEvent creates an updateFps event with fps payload', () => {
    const event = new UpdateFpsEvent(60)

    expect(event.type).toBe('updateFps')
    expect(event.fps).toBe(60)
  })
})
