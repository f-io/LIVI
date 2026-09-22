import { BACKUP_DIR, CONFIG_BACKUP_PATH, CONFIG_PATH } from '@main/config/paths'
import { homedir } from 'os'
import { join } from 'path'

describe('CONFIG_PATH', () => {
  test('points to config.json inside app userData', () => {
    expect(CONFIG_PATH).toBe('/tmp/config.json')
  })
})

describe('BACKUP_DIR', () => {
  test('is the one folder to carry to a new machine', () => {
    const expected =
      process.platform === 'darwin'
        ? '/tmp/backup'
        : join(process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share'), 'LIVI')
    expect(BACKUP_DIR).toBe(expected)
    expect(CONFIG_BACKUP_PATH).toBe(join(expected, 'config.json'))
  })

  test('never coincides with the live config', () => {
    expect(CONFIG_BACKUP_PATH).not.toBe(CONFIG_PATH)
  })
})

// The constant is resolved at import time from the platform, so the other host's branch is only
// reachable by re-importing the module under a stubbed platform.
describe('BACKUP_DIR per platform', () => {
  const realPlatform = process.platform

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: realPlatform })
    vi.unstubAllEnvs()
    vi.resetModules()
  })

  async function backupDirOn(platform: string, xdgDataHome = ''): Promise<string> {
    Object.defineProperty(process, 'platform', { value: platform })
    vi.stubEnv('XDG_DATA_HOME', xdgDataHome)
    vi.resetModules()
    return (await import('@main/config/paths')).BACKUP_DIR
  }

  test('sits beside userData on macOS', async () => {
    expect(await backupDirOn('darwin', '/xdg/data')).toBe('/tmp/backup')
  })

  test('follows XDG_DATA_HOME elsewhere', async () => {
    expect(await backupDirOn('linux', '/xdg/data')).toBe(join('/xdg/data', 'LIVI'))
  })

  test('falls back to ~/.local/share when XDG_DATA_HOME is unset', async () => {
    expect(await backupDirOn('linux')).toBe(join(homedir(), '.local', 'share', 'LIVI'))
  })
})
