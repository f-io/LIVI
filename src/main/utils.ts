import { NullDeleteKey, runtimeStateProps } from '@main/types'
import { broadcastToRenderers } from '@main/window/broadcast'
import type { Config } from '@shared/types'
import { app } from 'electron'
import { NULL_DELETES } from './constants'

export const isMacPlatform = () => process.platform === 'darwin'

export function applyNullDeletes(merged: Config, next: Partial<Config>) {
  const nextAny = next as Record<string, unknown>
  const mergedAny = merged as Record<string, unknown>

  for (const key of NULL_DELETES) {
    if (nextAny[key] === null) {
      delete mergedAny[key as NullDeleteKey]
    }
  }
}

export function sizesEqual(a: Config, b: Config) {
  const aw = Number(a.displayWidth) || 0
  const ah = Number(a.displayHeight) || 0
  const bw = Number(b.displayWidth) || 0
  const bh = Number(b.displayHeight) || 0
  return aw === bw && ah === bh
}

export function setFeatureFlags(flags: string[]) {
  app.commandLine.appendSwitch('enable-features', flags.join(','))
}

export function linuxPresetAngleVulkan() {
  app.commandLine.appendSwitch('use-gl', 'angle')
  app.commandLine.appendSwitch('use-angle', 'vulkan')

  setFeatureFlags(['Vulkan', 'VulkanFromANGLE', 'DefaultANGLEVulkan'])
  app.commandLine.appendSwitch('ozone-platform-hint', 'auto')
}

export function pushSettingsToRenderer(
  runtimeState: runtimeStateProps,
  override?: Partial<Config>
) {
  broadcastToRenderers('settings', { ...runtimeState.config, ...(override ?? {}) })
}
