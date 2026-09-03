import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
import type { Mock } from 'vitest'
import { checkAndInstallUdevRule, udevRuleExists } from '../udevRule'

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
          title: 'udev Rule Update',
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

    test('treats an existing rule as current via the version-0 fallback marker', async () => {
      existsFake(true)
      mockReadFileSync.mockImplementation(function (p: string) {
        if (typeof p === 'string' && p.endsWith('.rules.template')) {
          return 'SUBSYSTEM=="usb", ATTR{idVendor}=="1314", MODE="0660", OWNER="__USERNAME__"\n'
        }
        return '# LIVI-RULE-VERSION=0\nSUBSYSTEM=="usb"\n'
      })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
    })

    test('prompts for an update when the rule file vanishes between checks', async () => {
      let ruleChecks = 0
      mockExistsSync.mockImplementation(function (p: string) {
        if (typeof p === 'string' && p.endsWith('.rules.template')) return true
        ruleChecks += 1
        return ruleChecks === 1
      })
      mockShowMessageBox.mockResolvedValue({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenCalledWith(
        mockWindow,
        expect.objectContaining({ title: 'udev Rule Update' })
      )
    })

    test('treats an unreadable rule file as outdated', async () => {
      existsFake(true)
      mockReadFileSync.mockImplementation(function (p: string, enc?: string) {
        if (typeof p === 'string' && p.endsWith('.rules.template')) {
          return realFs.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
        }
        throw new Error('EACCES')
      })
      mockShowMessageBox.mockResolvedValue({ response: 1 })
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).toHaveBeenCalledWith(
        mockWindow,
        expect.objectContaining({ title: 'udev Rule Update' })
      )
    })

    test('bundles the touch filter into the install script when present', async () => {
      mockReadFileSync.mockImplementation(function (p: string, enc?: string) {
        if (typeof p === 'string' && p.endsWith('.rules.template')) {
          return realFs.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
        }
        if (typeof p === 'string' && p.endsWith('livi-touch-filter')) {
          return '#!/bin/sh\nexit 0\n'
        }
        return ''
      })
      await checkAndInstallUdevRule(mockWindow)
      const script = mockSpawn.mock.calls[0][1][2] as string
      expect(script).toContain('LIVI_FILTER_EOF')
      expect(script).toContain('chmod 0755')
    })

    test('installs without the touch filter when it cannot be read', async () => {
      mockReadFileSync.mockImplementation(function (p: string, enc?: string) {
        if (typeof p === 'string' && p.endsWith('.rules.template')) {
          return realFs.readFileSync(p, (enc as BufferEncoding) ?? 'utf8')
        }
        throw new Error('missing filter asset')
      })
      await checkAndInstallUdevRule(mockWindow)
      const script = mockSpawn.mock.calls[0][1][2] as string
      expect(script).not.toContain('LIVI_FILTER_EOF')
    })

    test('stringifies non-Error spawn failures in the error dialog', async () => {
      const proc = {
        on: vi.fn((event: string, cb: (arg: unknown) => void) => {
          if (event === 'error') setTimeout(() => cb('spawn exploded'), 0)
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
          title: 'Installation Failed',
          detail: expect.stringContaining('spawn exploded')
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

  test('install script writes the template content as it is', async () => {
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string
    const template = realFs.readFileSync(
      `${process.cwd()}/assets/linux/99-LIVI.rules.template`,
      'utf8'
    )
    expect(script).toContain(template.trim())
  })
})
