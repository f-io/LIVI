import { app } from 'electron'
import { NULL_DELETES } from './constants'
import { ExtraConfig } from './Globals'
import { NullDeleteKey, runtimeStateProps } from '@main/types'
import { getMainWindow } from '@main/window/createWindow'

export const isMacPlatform = () => process.platform === 'darwin'

export function applyNullDeletes(merged: ExtraConfig, next: Partial<ExtraConfig>) {
  const nextAny = next as Record<string, unknown>
  const mergedAny = merged as Record<string, unknown>

  for (const key of NULL_DELETES) {
    if (nextAny[key] === null) {
      delete mergedAny[key as NullDeleteKey]
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

export function pushSettingsToRenderer(
  runtimeState: runtimeStateProps,
  override?: Partial<ExtraConfig>
) {
  const mainWindow = getMainWindow()

  if (!mainWindow || mainWindow.isDestroyed()) return
  mainWindow.webContents.send('settings', { ...runtimeState.config, ...(override ?? {}) })
}
