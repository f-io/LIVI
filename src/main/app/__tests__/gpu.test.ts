import { app } from 'electron'
import { commonGpuToggles } from '@main/app/gpu'

describe('commonGpuToggles', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('applies expected chromium gpu flags', () => {
    commonGpuToggles()

    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('ignore-gpu-blocklist')
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('enable-gpu-rasterization')
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith(
      'disable-features',
      'UseChromeOSDirectVideoDecoder'
    )
  })
})
