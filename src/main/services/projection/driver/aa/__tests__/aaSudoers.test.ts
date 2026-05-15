import { EventEmitter } from 'node:events'

class MockChild extends EventEmitter {}

const execFileSyncMock = jest.fn()
const spawnMock = jest.fn()
const existsSyncMock = jest.fn()
const writeFileSyncMock = jest.fn()
const showMessageBoxMock = jest.fn()
const lastChild: { instance: MockChild | null } = { instance: null }

jest.mock('node:child_process', () => ({
  execFileSync: (...a: unknown[]) => execFileSyncMock(...a),
  spawn: (...a: unknown[]) => spawnMock(...a)
}))

jest.mock('node:fs', () => ({
  existsSync: (...a: unknown[]) => existsSyncMock(...a),
  writeFileSync: (...a: unknown[]) => writeFileSyncMock(...a)
}))

jest.mock('node:os', () => ({
  userInfo: () => ({ username: 'tester' })
}))

jest.mock('electron', () => ({
  app: { getPath: jest.fn(() => '/tmp/livi-test') },
  BrowserWindow: class {},
  dialog: {
    showMessageBox: (...a: unknown[]) => showMessageBoxMock(...a)
  }
}))

import type { BrowserWindow } from 'electron'
import { aaSudoersExists, checkAndInstallAaSudoers } from '../aaSudoers'

beforeEach(() => {
  execFileSyncMock.mockReset()
  spawnMock.mockReset()
  existsSyncMock.mockReset()
  writeFileSyncMock.mockReset()
  showMessageBoxMock.mockReset()
  lastChild.instance = null
  delete process.env.PKEXEC_UID
  delete process.env.SUDO_USER
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})

  spawnMock.mockImplementation(() => {
    const c = new MockChild()
    lastChild.instance = c
    return c
  })
})
afterEach(() => jest.restoreAllMocks())

describe('aaSudoersExists', () => {
  test('true when "sudo -l" output mentions LIVI_AA_BT', () => {
    execFileSyncMock.mockReturnValueOnce('Cmnd_Alias LIVI_AA_BT')
    expect(aaSudoersExists()).toBe(true)
  })

  test('true when "sudo -l" output mentions aa-bluetooth.py', () => {
    execFileSyncMock.mockReturnValueOnce('... aa-bluetooth.py ...')
    expect(aaSudoersExists()).toBe(true)
  })

  test('false when sudo -l throws and the sentinel file is missing', () => {
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockReturnValueOnce(false)
    expect(aaSudoersExists()).toBe(false)
  })

  test('true when the sentinel file is present', () => {
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockReturnValueOnce(true)
    expect(aaSudoersExists()).toBe(true)
  })

  test('false when existsSync throws', () => {
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockImplementationOnce(() => {
      throw new Error('not a fs')
    })
    expect(aaSudoersExists()).toBe(false)
  })
})

describe('checkAndInstallAaSudoers', () => {
  function mockPlatform(p: NodeJS.Platform): void {
    Object.defineProperty(process, 'platform', { value: p, configurable: true })
  }

  test('no-op on non-Linux platforms', async () => {
    mockPlatform('darwin')
    await checkAndInstallAaSudoers({} as BrowserWindow)
    expect(spawnMock).not.toHaveBeenCalled()
  })

  test('no-op when sudoers is already present', async () => {
    mockPlatform('linux')
    execFileSyncMock.mockReturnValueOnce('LIVI_AA_BT')
    await checkAndInstallAaSudoers({} as BrowserWindow)
    expect(showMessageBoxMock).not.toHaveBeenCalled()
  })

  test('warns and bails when pkexec is missing', async () => {
    mockPlatform('linux')
    // ruleActiveInSudo → false (throw)
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    // sentinelPath check → false
    existsSyncMock.mockReturnValueOnce(false)
    // pkexec which → throw
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no pkexec')
    })
    await checkAndInstallAaSudoers({} as BrowserWindow)
    expect(showMessageBoxMock).not.toHaveBeenCalled()
  })

  test('exits early when user clicks Skip', async () => {
    mockPlatform('linux')
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockReturnValueOnce(false)
    execFileSyncMock.mockReturnValueOnce('/usr/bin/pkexec\n') // pkexec which OK
    // Now python-path lookup runs — provide existsSync for /usr/bin/python3
    existsSyncMock.mockReturnValueOnce(true)
    // Show the dialog
    showMessageBoxMock.mockResolvedValueOnce({ response: 1 })

    await checkAndInstallAaSudoers({} as BrowserWindow)
    expect(spawnMock).not.toHaveBeenCalled()
  })

  test('happy path: dialog → install → sentinel → done dialog', async () => {
    mockPlatform('linux')
    // No prior rule, no sentinel
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockReturnValueOnce(false)
    execFileSyncMock.mockReturnValueOnce('/usr/bin/pkexec\n')
    existsSyncMock.mockReturnValueOnce(true) // python3 present
    // First dialog → Install
    showMessageBoxMock.mockResolvedValueOnce({ response: 0 })

    const p = checkAndInstallAaSudoers({} as BrowserWindow)
    // Drive spawn to resolve install
    await Promise.resolve()
    await Promise.resolve()
    lastChild.instance!.emit('close', 0)

    // Final "Done" dialog
    showMessageBoxMock.mockResolvedValueOnce({ response: 0 })
    await p
    expect(writeFileSyncMock).toHaveBeenCalled()
    expect(showMessageBoxMock).toHaveBeenCalledTimes(2)
  })

  test('install failure surfaces a failure dialog', async () => {
    mockPlatform('linux')
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('no sudo')
    })
    existsSyncMock.mockReturnValueOnce(false)
    execFileSyncMock.mockReturnValueOnce('/usr/bin/pkexec\n')
    existsSyncMock.mockReturnValueOnce(true)
    showMessageBoxMock.mockResolvedValueOnce({ response: 0 })

    const p = checkAndInstallAaSudoers({} as BrowserWindow)
    await Promise.resolve()
    await Promise.resolve()
    lastChild.instance!.emit('close', 1)
    showMessageBoxMock.mockResolvedValueOnce({ response: 0 })
    await p

    // Failure dialog
    const calls = showMessageBoxMock.mock.calls
    expect(calls[calls.length - 1][1].type).toBe('error')
  })
})
