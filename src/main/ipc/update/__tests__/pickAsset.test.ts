import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'

function setPlatform(platform: NodeJS.Platform): void {
  Object.defineProperty(process, 'platform', { value: platform })
}

function setArch(arch: string): void {
  Object.defineProperty(process, 'arch', { value: arch })
}

describe('pickAssetForPlatform', () => {
  const platform = process.platform
  const arch = process.arch

  beforeEach(() => {
    setPlatform(platform)
    setArch(arch)
  })

  test('returns empty object for non-array input', () => {
    expect(pickAssetForPlatform(null as unknown as never[])).toEqual({})
  })

  test('picks x64 AppImage on linux x64', () => {
    setPlatform('linux')
    setArch('x64')

    const result = pickAssetForPlatform([
      { name: 'LIVI-arm64.AppImage', browser_download_url: 'https://example.com/arm' } as never,
      { name: 'LIVI-x86_64.AppImage', browser_download_url: 'https://example.com/x64' } as never
    ])

    expect(result).toEqual({ url: 'https://example.com/x64' })
  })

  test('picks arm64 AppImage on linux arm64', () => {
    setPlatform('linux')
    setArch('arm64')

    const result = pickAssetForPlatform([
      {
        name: 'LIVI-aarch64.AppImage',
        browser_download_url: 'https://example.com/aarch64'
      } as never,
      { name: 'LIVI-x64.AppImage', browser_download_url: 'https://example.com/x64' } as never
    ])

    expect(result).toEqual({ url: 'https://example.com/aarch64' })
  })

  test('picks arm64/universal DMG on darwin arm64', () => {
    setPlatform('darwin')
    setArch('arm64')

    const result = pickAssetForPlatform([
      { name: 'LIVI-x64.dmg', browser_download_url: 'https://example.com/x64' } as never,
      { name: 'LIVI-universal.dmg', browser_download_url: 'https://example.com/universal' } as never
    ])

    expect(result).toEqual({ url: 'https://example.com/universal' })
  })

  test('returns empty object for unsupported platform', () => {
    setPlatform('win32')
    const result = pickAssetForPlatform([
      { name: 'LIVI-x86_64.AppImage', browser_download_url: 'https://example.com/x64' } as never
    ])

    expect(result).toEqual({})
  })
})
