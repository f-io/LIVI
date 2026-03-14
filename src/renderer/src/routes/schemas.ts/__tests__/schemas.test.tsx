jest.mock('@renderer/components/pages/settings/pages/camera', () => ({
  Camera: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/iconUploader/IconUploader', () => ({
  IconUploader: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/softwareUpdate/SoftwareUpdate', () => ({
  SoftwareUpdate: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/usbDongle/USBDongle', () => ({
  USBDongle: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/debug/Debug', () => ({
  Debug: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/About', () => ({
  About: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/Restart', () => ({
  Restart: () => null
}))
jest.mock('@renderer/components/pages/settings/pages/system/PowerOff', () => ({
  PowerOff: () => null
}))
jest.mock('@renderer/components/pages/settings/SettingsPage', () => ({
  SettingsPage: () => null
}))

import { audioSchema } from '../audioSchema'
import { appearanceSchema } from '../appearanceSchema'
import { generalSchema } from '../generalSchema'
import { settingsRoutes, settingsSchema } from '../schema'
import { systemSchema } from '../systemSchema'
import { videoSchema } from '../videoSchema'

describe('settings schemas', () => {
  test('root schemas have route type and children', () => {
    for (const s of [generalSchema, audioSchema, videoSchema, appearanceSchema, systemSchema]) {
      expect(s.type).toBe('route')
      expect(Array.isArray((s as any).children)).toBe(true)
      expect((s as any).children.length).toBeGreaterThan(0)
    }
  })

  test('audio value transform handles invalid and valid values', () => {
    if (audioSchema.type !== 'route') {
      throw new Error('audioSchema must be a route node')
    }
    const slider = (audioSchema.children as any[]).find((x) => x.path === 'audioVolume')
    expect(slider.valueTransform.toView(0.45)).toBe(45)
    expect(slider.valueTransform.fromView(25, 1)).toBe(0.25)
    expect(slider.valueTransform.fromView(Number.NaN, 0.8)).toBe(0.8)
    expect(slider.valueTransform.format(50)).toBe('50 %')
  })

  test('settings schema aggregates major sections and generates routes', () => {
    expect(settingsSchema.type).toBe('route')
    if (settingsSchema.type !== 'route') {
      throw new Error('settingsSchema must be a route node')
    }
    expect((settingsSchema.children as any[]).length).toBe(5)
    expect(settingsRoutes?.path).toBe('new-settings')
    expect(Array.isArray(settingsRoutes?.children)).toBe(true)
  })
})
