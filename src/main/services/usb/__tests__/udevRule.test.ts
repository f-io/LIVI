import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import os from 'os'
import { checkAndInstallUdevRule, udevRuleExists } from '../udevRule'

jest.mock('electron', () => ({
  BrowserWindow: jest.fn(),
  dialog: {
    showMessageBox: jest.fn(),
    showErrorBox: jest.fn()
  }
}))

jest.mock('child_process', () => ({
  execFileSync: jest.fn(),
  spawn: jest.fn()
}))

jest.mock('fs', () => {
  const real = jest.requireActual('fs') as typeof import('fs')
  return {
    existsSync: jest.fn(),
    readFileSync: jest.fn((p: string, enc?: string) => {
      if (typeof p === 'string' && p.endsWith('.rules.template')) {
        return real.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
      }
      return ''
    })
  }
})

describe('udevRule', () => {
  const originalPlatform = process.platform
  const mockExistsSync = fs.existsSync as jest.Mock
  const mockReadFileSync = fs.readFileSync as jest.Mock
  const mockExecFileSync = execFileSync as jest.Mock
  const mockSpawn = spawn as jest.Mock
  const mockShowMessageBox = dialog.showMessageBox as jest.Mock
  const mockWindow = {} as BrowserWindow

  const mkProc = (exitCode: number) => {
    const listeners: Record<string, (code: number) => void> = {}
    return {
      on: jest.fn((event: string, cb: (code: number) => void) => {
        listeners[event] = cb
        if (event === 'close') setTimeout(() => cb(exitCode), 0)
      })
    }
  }

  const realFs = jest.requireActual('fs') as typeof fs

  const ruleFileFake = (content = '') => {
    mockReadFileSync.mockImplementation((p: string, enc?: string) => {
      if (typeof p === 'string' && p.endsWith('.rules.template')) {
        return realFs.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
      }
      return content
    })
  }

  beforeEach(() => {
    jest.clearAllMocks()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    mockExistsSync.mockImplementation((p: string) => {
      if (typeof p === 'string' && p.endsWith('.rules.template')) return true
      return false
    })
    ruleFileFake('')
    mockExecFileSync.mockReturnValue(undefined)
    mockShowMessageBox.mockResolvedValue({ response: 0 })
    mockSpawn.mockReturnValue(mkProc(0))
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  const existsFake = (ruleFileExists: boolean) => {
    mockExistsSync.mockImplementation((p: string) => {
      if (typeof p === 'string' && p.endsWith('.rules.template')) return true
      return ruleFileExists
    })
  }

  describe('udevRuleExists', () => {
    test('returns true when rule file exists', () => {
      existsFake(true)
      expect(udevRuleExists()).toBe(true)
    })

    test('returns false when rule file does not exist', () => {
      existsFake(false)
      expect(udevRuleExists()).toBe(false)
    })

    test('returns false when existsSync throws', () => {
      mockExistsSync.mockImplementation(() => {
        throw new Error('permission denied')
      })
      expect(udevRuleExists()).toBe(false)
    })
  })

  describe('checkAndInstallUdevRule', () => {
    test('does nothing on non-linux platforms', async () => {
      Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
    })

    test('does nothing when rule file already exists with a current version marker', async () => {
      const template = realFs.readFileSync(
        `${process.cwd()}/assets/linux/99-LIVI.rules.template`,
        'utf8'
      )
      const marker = template.match(/^# LIVI-RULE-VERSION=\d+$/m)![0]
      existsFake(true)
      ruleFileFake(`${marker}\n...rest of file...`)
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
    })

    test('prompts for an upgrade when an outdated rule file is present', async () => {
      existsFake(true)
      ruleFileFake(
        'SUBSYSTEM=="usb", ATTR{idVendor}=="1314", ATTR{idProduct}=="152*", MODE="0660", OWNER="me"\n'
      )
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenCalledWith(
        mockWindow,
        expect.objectContaining({
          title: 'USB Permission Update',
          buttons: ['Update', 'Skip']
        })
      )
      expect(mockSpawn).toHaveBeenCalledWith(
        'pkexec',
        ['bash', '-c', expect.stringContaining('LIVI-RULE-VERSION=')],
        { stdio: 'ignore' }
      )
    })

    test('skips the upgrade when the user declines', async () => {
      existsFake(true)
      ruleFileFake('outdated content')
      mockShowMessageBox.mockResolvedValue({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockSpawn).not.toHaveBeenCalled()
    })

    test('does nothing when pkexec is not available', async () => {
      mockExecFileSync.mockImplementation(() => {
        throw new Error('not found')
      })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
    })

    test('does nothing when user clicks Skip', async () => {
      mockShowMessageBox.mockResolvedValue({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockSpawn).not.toHaveBeenCalled()
    })

    test('spawns pkexec when user clicks Install', async () => {
      await checkAndInstallUdevRule(mockWindow)
      expect(mockSpawn).toHaveBeenCalledWith('pkexec', ['bash', '-c', expect.any(String)], {
        stdio: 'ignore'
      })
    })

    test('shows success dialog after successful install', async () => {
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenCalledTimes(2)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({ type: 'info', title: 'Done' })
      )
    })

    test('shows error dialog when pkexec exits with non-zero code', async () => {
      mockSpawn.mockReturnValue(mkProc(127))
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({ type: 'error', title: 'Installation Failed' })
      )
    })

    test('shows error dialog when spawn emits an error', async () => {
      const proc = {
        on: jest.fn((event: string, cb: (arg: unknown) => void) => {
          if (event === 'error') setTimeout(() => cb(new Error('spawn failed')), 0)
        })
      }
      mockSpawn.mockReturnValue(proc)
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({ type: 'error', title: 'Installation Failed' })
      )
    })
  })

  test('uses PKEXEC_UID to resolve username when available', async () => {
    process.env.PKEXEC_UID = '1000'
    mockExecFileSync
      .mockReturnValueOnce(undefined) // which pkexec
      .mockReturnValueOnce('testuser\n') // id -nu 1000
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string
    expect(script).toContain('OWNER="testuser"')
    delete process.env.PKEXEC_UID
  })

  test('falls back to SUDO_USER when PKEXEC_UID is not set', async () => {
    delete process.env.PKEXEC_UID
    process.env.SUDO_USER = 'sudouser'
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string
    expect(script).toContain('OWNER="sudouser"')
    delete process.env.SUDO_USER
  })

  test('falls back to os.userInfo when neither PKEXEC_UID nor SUDO_USER is set', async () => {
    delete process.env.PKEXEC_UID
    delete process.env.SUDO_USER
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string
    expect(script).toContain('OWNER="')
  })

  test('install script writes the template content with username substituted', async () => {
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string

    const template = realFs.readFileSync(
      `${process.cwd()}/assets/linux/99-LIVI.rules.template`,
      'utf8'
    )
    const username = os.userInfo().username
    const rendered = template.replace(/__USERNAME__/g, username).trim()

    expect(script).not.toContain('__USERNAME__')
    expect(script).toContain(rendered)
  })
})
