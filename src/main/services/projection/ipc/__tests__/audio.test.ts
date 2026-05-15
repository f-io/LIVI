type IpcOnHandler = (evt: unknown, ...args: unknown[]) => void
const onHandlers = new Map<string, IpcOnHandler>()

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn(),
  registerIpcOn: (channel: string, handler: IpcOnHandler) => {
    onHandlers.set(channel, handler)
  }
}))

import { registerAudioIpc } from '../audio'

beforeEach(() => onHandlers.clear())

describe('audio ipc', () => {
  test('projection-set-volume forwards stream + volume', () => {
    const host = { setAudioStreamVolume: jest.fn(), setAudioVisualizerEnabled: jest.fn() }
    registerAudioIpc(host)
    onHandlers.get('projection-set-volume')!(null, { stream: 'music', volume: 0.5 })
    expect(host.setAudioStreamVolume).toHaveBeenCalledWith('music', 0.5)
  })

  test('projection-set-volume null payload still calls setter (with undefineds)', () => {
    const host = { setAudioStreamVolume: jest.fn(), setAudioVisualizerEnabled: jest.fn() }
    registerAudioIpc(host)
    onHandlers.get('projection-set-volume')!(null, null)
    expect(host.setAudioStreamVolume).toHaveBeenCalled()
  })

  test('projection-set-visualizer-enabled coerces to boolean', () => {
    const host = { setAudioStreamVolume: jest.fn(), setAudioVisualizerEnabled: jest.fn() }
    registerAudioIpc(host)
    onHandlers.get('projection-set-visualizer-enabled')!(null, 1)
    onHandlers.get('projection-set-visualizer-enabled')!(null, 0)
    expect(host.setAudioVisualizerEnabled).toHaveBeenCalledWith(true)
    expect(host.setAudioVisualizerEnabled).toHaveBeenCalledWith(false)
  })
})
