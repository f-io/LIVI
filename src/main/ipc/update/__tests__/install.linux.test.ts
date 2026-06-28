import { installOnLinuxFromFile } from '@main/ipc/update/install.linux'
import { promises as fsp } from 'fs'

vi.mock('fs', () => {
  const __m = {
    promises: {
      copyFile: vi.fn(() => Promise.resolve()),
      chmod: vi.fn(() => Promise.resolve()),
      rename: vi.fn(() => Promise.resolve())
    }
  }
  return { ...__m, default: __m }
})

describe('installOnLinuxFromFile', () => {
  const originalPlatform = process.platform
  const originalAppImage = process.env.APPIMAGE

  beforeEach(async () => {
    vi.clearAllMocks()
  })

  afterEach(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    process.env.APPIMAGE = originalAppImage
  })

  test('throws outside linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    await expect(installOnLinuxFromFile('/tmp/new.AppImage')).rejects.toThrow('Linux only')
  })

  test('throws if not running from AppImage', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    delete process.env.APPIMAGE
    await expect(installOnLinuxFromFile('/tmp/new.AppImage')).rejects.toThrow(
      'Not running from an AppImage'
    )
  })

  test('replaces the running AppImage in place via a temp file', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/opt/LIVI.AppImage'

    await installOnLinuxFromFile('/tmp/downloaded.AppImage')

    expect(fsp.copyFile).toHaveBeenCalledWith('/tmp/downloaded.AppImage', '/opt/LIVI.AppImage.new')
    expect(fsp.chmod).toHaveBeenCalledWith('/opt/LIVI.AppImage.new', 0o755)
    expect(fsp.rename).toHaveBeenCalledWith('/opt/LIVI.AppImage.new', '/opt/LIVI.AppImage')
  })
})
