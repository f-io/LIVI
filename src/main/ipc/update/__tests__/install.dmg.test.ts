import { execFile } from 'node:child_process'
import { promises as fsp } from 'fs'
import { getMacDesiredOwner, sendUpdateEvent } from '@main/ipc/utils'
import { installFromDmg } from '@main/ipc/update/install.dmg'

jest.mock('node:child_process', () => ({
  execFile: jest.fn()
}))

jest.mock('fs', () => ({
  promises: {
    readdir: jest.fn()
  }
}))

jest.mock('@main/ipc/utils', () => ({
  getMacDesiredOwner: jest.fn(() => Promise.resolve({ user: 'anton', group: 'staff' })),
  sendUpdateEvent: jest.fn()
}))

describe('installFromDmg', () => {
  const originalPlatform = process.platform

  beforeEach(() => {
    jest.clearAllMocks()
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  test('throws outside macOS', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    await expect(installFromDmg('/tmp/LIVI.dmg')).rejects.toThrow('macOS only')
  })

  test('mounts dmg, copies app via osascript and unmounts', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    ;(execFile as jest.Mock).mockImplementation((cmd, args, cb) => cb(null))
    ;(fsp.readdir as jest.Mock).mockResolvedValue([
      {
        name: 'LIVI.app',
        isDirectory: () => true
      }
    ])

    await expect(installFromDmg('/tmp/LIVI.dmg')).resolves.toBeUndefined()

    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'mounting' })
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'copying' })
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'unmounting' })
    expect(getMacDesiredOwner).toHaveBeenCalledWith('/Applications/LIVI.app')
    expect(execFile).toHaveBeenCalledWith(
      'hdiutil',
      expect.arrayContaining(['attach', '-nobrowse']),
      expect.any(Function)
    )
    expect(execFile).toHaveBeenCalledWith('osascript', expect.any(Array), expect.any(Function))
    expect(execFile).toHaveBeenCalledWith(
      'hdiutil',
      expect.arrayContaining(['detach', expect.stringMatching(/^\/Volumes\/pcu-/), '-quiet']),
      expect.any(Function)
    )
  })

  test('detaches and throws when no .app found in dmg', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    ;(execFile as jest.Mock).mockImplementation((cmd, args, cb) => cb(null))
    ;(fsp.readdir as jest.Mock).mockResolvedValue([
      {
        name: 'README.txt',
        isDirectory: () => false
      }
    ])

    await expect(installFromDmg('/tmp/LIVI.dmg')).rejects.toThrow('No .app found in DMG')
    expect(execFile).toHaveBeenCalledWith(
      'hdiutil',
      expect.arrayContaining(['detach', expect.any(String), '-quiet']),
      expect.any(Function)
    )
  })
})
