import { app } from 'electron'
import { linuxPresetAngleVulkan } from '../utils'

// Linux x64 -> ANGLE + Vulkan + WebGPU
if (process.platform === 'linux' && process.arch === 'x64') {
  commonGpuToggles()
  linuxPresetAngleVulkan()
  app.commandLine.appendSwitch('enable-unsafe-webgpu')
  app.commandLine.appendSwitch('enable-dawn-features', 'allow_unsafe_apis')
}

// macOS: WebGPU
if (process.platform === 'darwin') {
  app.commandLine.appendSwitch('enable-unsafe-webgpu')
  app.commandLine.appendSwitch('enable-dawn-features', 'allow_unsafe_apis')
}

app.on('gpu-info-update', () => {
  console.log('GPU Info:', app.getGPUFeatureStatus())
})

export function commonGpuToggles() {
  app.commandLine.appendSwitch('ignore-gpu-blocklist')
  app.commandLine.appendSwitch('enable-gpu-rasterization')
  app.commandLine.appendSwitch('disable-features', 'UseChromeOSDirectVideoDecoder')
}
