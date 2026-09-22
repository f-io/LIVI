import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'

const fs = vi.hoisted(() => ({
  existsSync: vi.fn(),
  readFileSync: vi.fn(),
  writeFileSync: vi.fn(),
  mkdirSync: vi.fn()
}))
const execFile = vi.hoisted(() => vi.fn())

vi.mock('node:fs', () => fs)
vi.mock('node:child_process', () => ({ execFile }))
vi.mock('node:os', () => ({ default: { homedir: () => '/home/pi' }, homedir: () => '/home/pi' }))

import { ensureWireplumberBtRoles } from '../wireplumberBtRoles'

const CONTENT = `monitor.bluez.properties = {
  bluez5.roles = [ a2dp_sink a2dp_source ]
}
`

describe('ensureWireplumberBtRoles', () => {
  let realPlatform: PropertyDescriptor | undefined
  beforeEach(() => {
    realPlatform = Object.getOwnPropertyDescriptor(process, 'platform')
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    vi.clearAllMocks()
  })
  afterEach(() => {
    if (realPlatform) Object.defineProperty(process, 'platform', realPlatform)
  })

  test('writes the drop-in and restarts wireplumber when absent', () => {
    fs.existsSync.mockReturnValue(false)
    execFile.mockImplementation((_c: string, _a: string[], cb: any) => cb(null))
    ensureWireplumberBtRoles()
    expect(fs.writeFileSync).toHaveBeenCalledWith(
      expect.stringContaining('99-livi-hfp.conf'),
      CONTENT
    )
    expect(execFile).toHaveBeenCalledWith(
      'systemctl',
      ['--user', 'restart', 'wireplumber'],
      expect.any(Function)
    )
  })

  test('does nothing when the drop-in already matches', () => {
    fs.existsSync.mockReturnValue(true)
    fs.readFileSync.mockReturnValue(CONTENT)
    ensureWireplumberBtRoles()
    expect(fs.writeFileSync).not.toHaveBeenCalled()
    expect(execFile).not.toHaveBeenCalled()
  })

  test('logs but does not throw when the restart fails', () => {
    fs.existsSync.mockReturnValue(false)
    execFile.mockImplementation((_c: string, _a: string[], cb: any) => cb(new Error('no bus')))
    expect(() => ensureWireplumberBtRoles()).not.toThrow()
    expect(execFile).toHaveBeenCalled()
  })

  test('swallows a write failure', () => {
    fs.existsSync.mockReturnValue(false)
    fs.writeFileSync.mockImplementationOnce(() => {
      throw new Error('read-only fs')
    })
    expect(() => ensureWireplumberBtRoles()).not.toThrow()
    expect(execFile).not.toHaveBeenCalled()
  })

  test('is a no-op off linux', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    ensureWireplumberBtRoles()
    expect(fs.writeFileSync).not.toHaveBeenCalled()
  })
})
