import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import os from 'os'
import type { Mock } from 'vitest'
import {
  checkAndInstallUdevRule,
  phoneVendorIdsFromUdevTemplate,
  udevRuleExists
} from '../udevRule'

vi.mock('electron', () => ({
  BrowserWindow: vi.fn(),
  dialog: {
    showMessageBox: vi.fn(),
    showErrorBox: vi.fn()
  }
}))

vi.mock('child_process', () => ({
  execFileSync: vi.fn(),
  spawn: vi.fn()
}))

vi.mock('fs', async () => {
  const real = (await vi.importActual('fs')) as typeof import('fs')
  const mock = {
    existsSync: vi.fn(),
    readFileSync: vi.fn(function (p: string, enc?: string) {
      if (typeof p === 'string' && p.endsWith('.rules.template')) {
        return real.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
      }
      return ''
    })
  }
  return { ...mock, default: mock }
})

describe('udevRule', () => {
  const originalPlatform = process.platform
  const mockExistsSync = fs.existsSync as Mock
  const mockReadFileSync = fs.readFileSync as Mock
  const mockExecFileSync = execFileSync as Mock
  const mockSpawn = spawn as Mock
  const mockShowMessageBox = dialog.showMessageBox as Mock
  const mockWindow = {} as BrowserWindow

  const mkProc = (exitCode: number) => {
    const listeners: Record<string, (code: number) => void> = {}
    return {
      on: vi.fn((event: string, cb: (code: number) => void) => {
        listeners[event] = cb
        if (event === 'close') setTimeout(() => cb(exitCode), 0)
      })
    }
  }

  let realFs: typeof fs
  beforeAll(async () => {
    realFs = (await vi.importActual('fs')) as typeof fs
  })

  const ruleFileFake = (content = '') => {
    mockReadFileSync.mockImplementation(function (p: string, enc?: string) {
      if (typeof p === 'string' && p.endsWith('.rules.template')) {
        return realFs.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
      }
      return content
    })
  }

  beforeEach(async () => {
    vi.clearAllMocks()
    vi.spyOn(os, 'userInfo').mockReturnValue({
      username: 'testuser',
      uid: 1000,
      gid: 1000,
      shell: '/bin/sh',
      homedir: '/home/testuser'
    })
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    mockExistsSync.mockImplementation(function (p: string) {
      if (typeof p === 'string' && p.endsWith('.rules.template')) return true
      return false
    })
    ruleFileFake('')
    mockExecFileSync.mockReturnValue(undefined)
    mockShowMessageBox.mockResolvedValue({ response: 0 })
    mockSpawn.mockReturnValue(mkProc(0))
  })

  afterEach(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  const existsFake = (ruleFileExists: boolean) => {
    mockExistsSync.mockImplementation(function (p: string) {
      if (typeof p === 'string' && p.endsWith('.rules.template')) return true
      return ruleFileExists
    })
  }

  describe('udevRuleExists', () => {
    test('returns true when rule file exists', async () => {
      existsFake(true)
      expect(udevRuleExists()).toBe(true)
    })

    test('returns false when rule file does not exist', async () => {
      existsFake(false)
      expect(udevRuleExists()).toBe(false)
    })

    test('returns false when existsSync throws', async () => {
      mockExistsSync.mockImplementation(function () {
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

    test('prefers the packaged template path when resourcesPath is set', async () => {
      const template = realFs.readFileSync(
        `${process.cwd()}/assets/linux/99-LIVI.rules.template`,
        'utf8'
      )
      mockReadFileSync.mockReturnValue(template)
      Object.defineProperty(process, 'resourcesPath', {
        value: '/opt/livi/resources',
        configurable: true
      })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockExistsSync).toHaveBeenCalledWith('/opt/livi/resources/99-LIVI.rules.template')
      Reflect.deleteProperty(process, 'resourcesPath')
    })

    test('does nothing when pkexec is not available', async () => {
      mockExecFileSync.mockImplementation(function () {
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

    test('shows error dialog with Retry/Skip when pkexec exits with non-zero code', async () => {
      mockSpawn.mockReturnValue(mkProc(127))
      mockShowMessageBox
        .mockResolvedValueOnce({ response: 0 })
        .mockResolvedValueOnce({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({
          type: 'error',
          title: 'Installation Failed',
          buttons: ['Retry', 'Skip']
        })
      )
    })

    test('shows error dialog with Retry/Skip when spawn emits an error', async () => {
      const proc = {
        on: vi.fn((event: string, cb: (arg: unknown) => void) => {
          if (event === 'error') setTimeout(() => cb(new Error('spawn failed')), 0)
        })
      }
      mockSpawn.mockReturnValue(proc)
      mockShowMessageBox
        .mockResolvedValueOnce({ response: 0 })
        .mockResolvedValueOnce({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({
          type: 'error',
          title: 'Installation Failed',
          buttons: ['Retry', 'Skip']
        })
      )
    })

    test('retries the install and shows success when the user clicks Retry', async () => {
      mockSpawn.mockReturnValueOnce(mkProc(127)).mockReturnValueOnce(mkProc(0))
      mockShowMessageBox
        .mockResolvedValueOnce({ response: 0 })
        .mockResolvedValueOnce({ response: 0 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockSpawn).toHaveBeenCalledTimes(2)
      expect(mockShowMessageBox).toHaveBeenLastCalledWith(
        mockWindow,
        expect.objectContaining({ type: 'info', title: 'Done' })
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

  describe('phoneVendorIdsFromUdevTemplate', () => {
    test('parses the phone vendor allowlist from the template and caches it', () => {
      const ids = phoneVendorIdsFromUdevTemplate()
      expect(ids).toBeInstanceOf(Set)
      expect(ids!.size).toBeGreaterThan(0)
      expect(ids!.has(0x1314)).toBe(false)
      expect(phoneVendorIdsFromUdevTemplate()).toBe(ids)
    })
  })
})
