import { execFileSync, spawn } from 'child_process'
import { BrowserWindow, dialog } from 'electron'
import fs from 'fs'
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

jest.mock('fs', () => ({
  existsSync: jest.fn()
}))

describe('udevRule', () => {
  const originalPlatform = process.platform
  const mockExistsSync = fs.existsSync as jest.Mock
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

  beforeEach(() => {
    jest.clearAllMocks()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    mockExistsSync.mockReturnValue(false)
    mockExecFileSync.mockReturnValue(undefined)
    mockShowMessageBox.mockResolvedValue({ response: 0 })
    mockSpawn.mockReturnValue(mkProc(0))
  })

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  describe('udevRuleExists', () => {
    test('returns true when rule file exists', () => {
      mockExistsSync.mockReturnValue(true)
      expect(udevRuleExists()).toBe(true)
    })

    test('returns false when rule file does not exist', () => {
      mockExistsSync.mockReturnValue(false)
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

    test('does nothing when rule file already exists', async () => {
      mockExistsSync.mockReturnValue(true)
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
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
})
