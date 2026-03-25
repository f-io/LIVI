import type { ExtraConfig } from '@shared/types'
import { updateCameras } from '../cameraDetection'

describe('updateCameras', () => {
  const originalMediaDevices = navigator.mediaDevices

  const makeVideoInput = (deviceId: string, label = 'Camera'): MediaDeviceInfo =>
    ({
      deviceId,
      groupId: '',
      kind: 'videoinput',
      label,
      toJSON: () => ({})
    }) as MediaDeviceInfo

  const makeAudioInput = (deviceId: string, label = 'Mic'): MediaDeviceInfo =>
    ({
      deviceId,
      groupId: '',
      kind: 'audioinput',
      label,
      toJSON: () => ({})
    }) as MediaDeviceInfo

  beforeEach(() => {
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: {
        enumerateDevices: jest.fn()
      }
    })
  })

  afterAll(() => {
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: originalMediaDevices
    })
  })

  test('returns only video input devices and sets camera found true', async () => {
    const setCameraFound = jest.fn()
    const saveSettings = jest.fn()

    ;(navigator.mediaDevices.enumerateDevices as jest.Mock).mockResolvedValue([
      makeAudioInput('mic-1'),
      makeVideoInput('cam-1', 'Camera 1'),
      makeVideoInput('cam-2', 'Camera 2')
    ])

    const currentSettings = {} as ExtraConfig

    const result = await updateCameras(setCameraFound, saveSettings, currentSettings)

    expect(result).toHaveLength(2)
    expect(result[0]).toMatchObject({
      kind: 'videoinput',
      deviceId: 'cam-1',
      label: 'Camera 1'
    })
    expect(result[1]).toMatchObject({
      kind: 'videoinput',
      deviceId: 'cam-2',
      label: 'Camera 2'
    })
    expect(setCameraFound).toHaveBeenCalledWith(true)
  })

  test('saves first detected camera when no current camera is set', async () => {
    const setCameraFound = jest.fn()
    const saveSettings = jest.fn()

    ;(navigator.mediaDevices.enumerateDevices as jest.Mock).mockResolvedValue([
      makeVideoInput('cam-1'),
      makeVideoInput('cam-2')
    ])

    const currentSettings = {
      nightMode: false
    } as ExtraConfig

    await updateCameras(setCameraFound, saveSettings, currentSettings)

    expect(saveSettings).toHaveBeenCalledWith({
      ...currentSettings,
      camera: 'cam-1'
    })
  })

  test('does not save settings when camera is already configured', async () => {
    const setCameraFound = jest.fn()
    const saveSettings = jest.fn()

    ;(navigator.mediaDevices.enumerateDevices as jest.Mock).mockResolvedValue([
      makeVideoInput('cam-1')
    ])

    const currentSettings = {
      camera: 'existing-cam'
    } as ExtraConfig

    await updateCameras(setCameraFound, saveSettings, currentSettings)

    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('sets camera found false and does not save when no cameras exist', async () => {
    const setCameraFound = jest.fn()
    const saveSettings = jest.fn()

    ;(navigator.mediaDevices.enumerateDevices as jest.Mock).mockResolvedValue([
      makeAudioInput('mic-1')
    ])

    const currentSettings = {} as ExtraConfig

    const result = await updateCameras(setCameraFound, saveSettings, currentSettings)

    expect(result).toEqual([])
    expect(setCameraFound).toHaveBeenCalledWith(false)
    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('returns empty array and warns when enumerateDevices fails', async () => {
    const setCameraFound = jest.fn()
    const saveSettings = jest.fn()
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

    const error = new Error('enumerate failed')

    ;(navigator.mediaDevices.enumerateDevices as jest.Mock).mockRejectedValue(error)

    const currentSettings = {} as ExtraConfig

    const result = await updateCameras(setCameraFound, saveSettings, currentSettings)

    expect(result).toEqual([])
    expect(setCameraFound).not.toHaveBeenCalled()
    expect(saveSettings).not.toHaveBeenCalled()
    expect(warnSpy).toHaveBeenCalledWith('[CameraDetection] enumerateDevices failed', error)

    warnSpy.mockRestore()
  })
})
