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
  existsSync: jest.fn(),
  readFileSync: jest.fn()
}))

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

  beforeEach(() => {
    jest.clearAllMocks()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    mockExistsSync.mockReturnValue(false)
    mockReadFileSync.mockReturnValue('')
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

    test('does nothing when rule file already exists with a current version marker', async () => {
      mockExistsSync.mockReturnValue(true)
      mockReadFileSync.mockReturnValue('# LIVI-RULE-VERSION=3\n...rest of file...')
      await checkAndInstallUdevRule(mockWindow)
      expect(mockShowMessageBox).not.toHaveBeenCalled()
    })

    test('prompts for an upgrade when an outdated rule file is present', async () => {
      mockExistsSync.mockReturnValue(true)
      mockReadFileSync.mockReturnValue(
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
      mockExistsSync.mockReturnValue(true)
      mockReadFileSync.mockReturnValue('outdated content')
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

  test('rule covers the Carlinkit dongle, AOAP accessory PIDs and common phone vendors', async () => {
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string

    // Carlinkit dongle
    expect(script).toContain('ATTR{idVendor}=="1314", ATTR{idProduct}=="152*"')
    // Android Open Accessory Protocol re-enumeration PIDs
    expect(script).toContain('ATTR{idVendor}=="18d1", ATTR{idProduct}=="2d0[0-5]"')
    // A representative slice of phone vendors
    for (const vid of ['18d1', '04e8', '22b8', '0fce', '12d1', '2717', '2a70']) {
      expect(script).toContain(`ATTR{idVendor}=="${vid}"`)
    }
  })

  test('phone and accessory entries opt out of desktop auto-mount (gvfs-mtp, udisks2, ModemManager)', async () => {
    await checkAndInstallUdevRule(mockWindow)
    const script = mockSpawn.mock.calls[0][1][2] as string

    // The desktop ignore env vars prevent the "Couldn't find matching udev
    // device" notification that gvfs-mtp pops when the phone switches to
    // accessory mode mid-mount.
    expect(script).toContain('ENV{ID_MTP_DEVICE}=""')
    expect(script).toContain('ENV{ID_MEDIA_PLAYER}=""')
    expect(script).toContain('ENV{UDISKS_IGNORE}="1"')
    expect(script).toContain('ENV{ID_MM_DEVICE_IGNORE}="1"')
  })
})
