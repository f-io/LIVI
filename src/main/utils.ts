import { app } from 'electron'
import { NULL_DELETES } from './constants'
import { ExtraConfig } from './Globals'

export const isMacPlatform = () => process.platform === 'darwin'

export function applyNullDeletes(merged: ExtraConfig, next: Partial<ExtraConfig>) {
  for (const key of NULL_DELETES) {
    if ((next as any)[key] === null) {
      delete (merged as any)[key]
    }
  }
}

export function sizesEqual(a: ExtraConfig, b: ExtraConfig) {
  const aw = Number(a.width) || 0
  const ah = Number(a.height) || 0
  const bw = Number(b.width) || 0
  const bh = Number(b.height) || 0
  return aw === bw && ah === bh
}

export function setFeatureFlags(flags: string[]) {
  app.commandLine.appendSwitch('enable-features', flags.join(','))
}

export function linuxPresetAngleVulkan() {
  app.commandLine.appendSwitch('use-gl', 'angle')
  app.commandLine.appendSwitch('use-angle', 'vulkan')
  setFeatureFlags([
    'Vulkan',
    'VulkanFromANGLE',
    'DefaultANGLEVulkan',
    'UnsafeWebGPU',
    'AcceleratedVideoDecodeLinuxZeroCopyGL',
    'AcceleratedVideoEncoder',
    'VaapiIgnoreDriverChecks',
    'UseMultiPlaneFormatForHardwareVideo'
  ])
  app.commandLine.appendSwitch('ozone-platform-hint', 'auto')
}
