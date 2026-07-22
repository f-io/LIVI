import { registerIpcHandle } from '@main/ipc/register'
import { registerSettingsIpc } from '@main/ipc/settings'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { currentKiosk } from '@main/window/utils'
import { app } from 'electron'
import type { Mock } from 'vitest'

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: vi.fn()
}))

vi.mock('@main/window/utils', () => ({
  currentKiosk: vi.fn(() => true)
}))

vi.mock('@main/ipc/update/pickAsset', () => ({
  pickAssetForPlatform: vi.fn(function () {
    return { url: 'https://example.com/LIVI.AppImage' }
  })
}))

vi.mock('@main/ipc/utils', () => ({
  configEvents: { on: vi.fn() },
  saveSettings: vi.fn()
}))

describe('registerSettingsIpc', () => {
  const runtimeState = { config: { kiosk: true } } as never

  beforeEach(async () => {
    vi.clearAllMocks()
  })

  function getHandler<T = (...args: unknown[]) => unknown>(channel: string): T {
    const pair = (registerIpcHandle as Mock).mock.calls.find(([ch]) => ch === channel)
    if (!pair) throw new Error(`Handler not registered for ${channel}`)
    return pair[1] as T
  }

  test('registers all expected settings IPC handlers', async () => {
    registerSettingsIpc(runtimeState)

    const channels = (registerIpcHandle as Mock).mock.calls.map(([ch]) => ch)
    expect(channels).toEqual(
      expect.arrayContaining([
        'settings:get-kiosk',
        'getSettings',
        'save-settings',
        'settings:reset-dongle-icons',
        'app:getVersion',
        'app:getLatestRelease'
      ])
    )
    expect(configEvents.on).toHaveBeenCalledWith('requestSave', expect.any(Function))
  })

  test('save-settings handler delegates to saveSettings and returns true', async () => {
    registerSettingsIpc(runtimeState)
    const handler =
      getHandler<(_evt: unknown, payload: Record<string, unknown>) => boolean>('save-settings')

    const patch = { language: 'de' }
    const result = handler({}, patch)

    expect(saveSettings).toHaveBeenCalledWith(runtimeState, patch)
    expect(result).toBe(true)
  })

  test('settings:get-kiosk returns currentKiosk(runtimeState.config)', async () => {
    registerSettingsIpc(runtimeState)
    const handler = getHandler<() => boolean>('settings:get-kiosk')

    expect(handler()).toBe(true)
    expect(currentKiosk).toHaveBeenCalledWith(runtimeState.config)
  })

  test('app:getVersion returns electron app version', async () => {
    ;(app.getVersion as Mock).mockReturnValue('9.9.9')

    registerSettingsIpc(runtimeState)
    const handler = getHandler<() => string>('app:getVersion')

    expect(handler()).toBe('9.9.9')
    expect(app.getVersion).toHaveBeenCalledTimes(1)
  })

  test('app:getLatestRelease normalizes version and picks platform asset', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        tag_name: 'v1.2.3',
        assets: [{ name: 'LIVI-x86_64.AppImage', browser_download_url: 'https://example.com/a' }]
      })
    })
    ;(global as any).fetch = fetchMock

    registerSettingsIpc(runtimeState)
    const handler =
      getHandler<() => Promise<{ version: string; url?: string }>>('app:getLatestRelease')

    const result = await handler()

    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/releases/latest'), {
      headers: { 'User-Agent': 'LIVI-updater' }
    })
    expect(pickAssetForPlatform).toHaveBeenCalled()
    expect(result).toEqual({
      version: '1.2.3',
      url: 'https://example.com/LIVI.AppImage',
      commit: '',
      run: ''
    })
  })

  test('app:getLatestRelease returns empty payload when fetch fails', async () => {
    const fetchMock = vi.fn().mockResolvedValue({ ok: false, status: 500 })
    ;(global as any).fetch = fetchMock
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    registerSettingsIpc(runtimeState)
    const handler =
      getHandler<() => Promise<{ version: string; url?: string }>>('app:getLatestRelease')

    await expect(handler()).resolves.toEqual({
      version: '',
      url: undefined,
      commit: '',
      run: ''
    })
    expect(warnSpy).toHaveBeenCalled()

    warnSpy.mockRestore()
  })

  test('requestSave event handler delegates to saveSettings', async () => {
    registerSettingsIpc(runtimeState)

    const requestSaveHandler = (configEvents.on as Mock).mock.calls.find(
      ([event]) => event === 'requestSave'
    )?.[1] as ((settings: Partial<Record<string, unknown>>) => void) | undefined

    if (!requestSaveHandler) {
      throw new Error('requestSave handler not registered')
    }

    const patch = { language: 'uk' }
    requestSaveHandler(patch)

    expect(saveSettings).toHaveBeenCalledWith(runtimeState, patch)
  })

  test('settings:reset-dongle-icons restores bundled icon defaults and returns them', async () => {
    const richRuntimeState = {
      config: {
        kiosk: true,
        dongleIcon120: 'old-120',
        dongleIcon180: 'old-180',
        dongleIcon256: 'old-256'
      }
    } as never

    registerSettingsIpc(richRuntimeState)

    const handler = getHandler<
      () => {
        dongleIcon120: string
        dongleIcon180: string
        dongleIcon256: string
      }
    >('settings:reset-dongle-icons')

    const result = handler()

    expect(saveSettings).toHaveBeenCalledWith(
      richRuntimeState,
      expect.objectContaining({
        dongleIcon120: expect.any(String),
        dongleIcon180: expect.any(String),
        dongleIcon256: expect.any(String)
      })
    )

    expect(result).toEqual(
      expect.objectContaining({
        dongleIcon120: expect.any(String),
        dongleIcon180: expect.any(String),
        dongleIcon256: expect.any(String)
      })
    )
  })

  test('app:getLatestRelease falls back to json.name when tag_name is missing', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        name: 'v2.3.4',
        assets: [{ name: 'LIVI-x86_64.AppImage', browser_download_url: 'https://example.com/a' }]
      })
    })
    ;(global as any).fetch = fetchMock

    registerSettingsIpc(runtimeState)
    const handler =
      getHandler<() => Promise<{ version: string; url?: string }>>('app:getLatestRelease')

    const result = await handler()

    expect(pickAssetForPlatform).toHaveBeenCalledWith([
      { name: 'LIVI-x86_64.AppImage', browser_download_url: 'https://example.com/a' }
    ])
    expect(result).toEqual({
      version: '2.3.4',
      url: 'https://example.com/LIVI.AppImage',
      commit: '',
      run: ''
    })
  })

  test('app:getLatestRelease falls back to empty version and empty assets array', async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({})
    })
    ;(global as any).fetch = fetchMock
    ;(pickAssetForPlatform as Mock).mockReturnValueOnce({ url: undefined })

    registerSettingsIpc(runtimeState)
    const handler =
      getHandler<() => Promise<{ version: string; url?: string }>>('app:getLatestRelease')

    const result = await handler()

    expect(pickAssetForPlatform).toHaveBeenCalledWith([])
    expect(result).toEqual({ version: '', url: undefined, commit: '', run: '' })
  })
})
