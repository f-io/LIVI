import { linuxPresetAngleVulkan } from '@main/utils'
import { app } from 'electron'

jest.mock('electron', () => ({
  app: {
    commandLine: {
      appendSwitch: jest.fn()
    }
  }
}))

jest.mock('@main/utils', () => ({
  linuxPresetAngleVulkan: jest.fn()
}))

const mockedAppendSwitch = app.commandLine.appendSwitch as jest.Mock
const mockedLinuxPresetAngleVulkan = linuxPresetAngleVulkan as jest.Mock

describe('gpu module', () => {
  const originalPlatform = process.platform
  const originalArch = process.arch

  const loadGpuModule = () => {
    jest.isolateModules(() => {
      require('@main/app/gpu')
    })
  }

  beforeEach(() => {
    jest.clearAllMocks()
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    Object.defineProperty(process, 'arch', { value: originalArch })
  })

  test('commonGpuToggles applies expected chromium gpu flags', () => {
    Object.defineProperty(process, 'platform', { value: 'win32' })
    Object.defineProperty(process, 'arch', { value: 'x64' })

    let commonGpuToggles: () => void

    jest.isolateModules(() => {
      ;({ commonGpuToggles } = require('@main/app/gpu'))
    })

    mockedAppendSwitch.mockClear()
    commonGpuToggles()

    expect(mockedAppendSwitch).toHaveBeenCalledWith('ignore-gpu-blocklist')
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-gpu-rasterization')
    expect(mockedAppendSwitch).toHaveBeenCalledWith(
      'disable-features',
      'UseChromeOSDirectVideoDecoder'
    )
  })

  test('on linux x64 import applies gpu toggles, linux preset and webgpu flags', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'x64' })

    loadGpuModule()

    expect(mockedAppendSwitch).toHaveBeenCalledWith('ignore-gpu-blocklist')
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-gpu-rasterization')
    expect(mockedAppendSwitch).toHaveBeenCalledWith(
      'disable-features',
      'UseChromeOSDirectVideoDecoder'
    )
    expect(mockedLinuxPresetAngleVulkan).toHaveBeenCalledTimes(1)
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-unsafe-webgpu')
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-dawn-features', 'allow_unsafe_apis')
  })

  test('on linux non-x64 import does not apply linux gpu preset side effects', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    Object.defineProperty(process, 'arch', { value: 'arm64' })

    loadGpuModule()

    expect(mockedLinuxPresetAngleVulkan).not.toHaveBeenCalled()
    expect(mockedAppendSwitch).not.toHaveBeenCalledWith('enable-unsafe-webgpu')
    expect(mockedAppendSwitch).not.toHaveBeenCalledWith('enable-dawn-features', 'allow_unsafe_apis')
  })

  test('on darwin import applies webgpu flags only', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    Object.defineProperty(process, 'arch', { value: 'arm64' })

    loadGpuModule()

    expect(mockedLinuxPresetAngleVulkan).not.toHaveBeenCalled()
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-unsafe-webgpu')
    expect(mockedAppendSwitch).toHaveBeenCalledWith('enable-dawn-features', 'allow_unsafe_apis')
    expect(mockedAppendSwitch).not.toHaveBeenCalledWith('ignore-gpu-blocklist')
  })

  test('on unsupported platform import does not apply startup gpu side effects', () => {
    Object.defineProperty(process, 'platform', { value: 'win32' })
    Object.defineProperty(process, 'arch', { value: 'x64' })

    loadGpuModule()

    expect(mockedLinuxPresetAngleVulkan).not.toHaveBeenCalled()
    expect(mockedAppendSwitch).not.toHaveBeenCalled()
  })
})
