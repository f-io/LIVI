import { app } from 'electron'
import { installFromDmg } from '@main/ipc/update/install.dmg'
import { sendUpdateEvent } from '@main/ipc/utils'
import { installOnMacFromFile } from '@main/ipc/update/install.mac'

jest.mock('@main/ipc/update/install.dmg', () => ({
  installFromDmg: jest.fn(() => Promise.resolve())
}))

jest.mock('@main/ipc/utils', () => ({
  sendUpdateEvent: jest.fn()
}))

describe('installOnMacFromFile', () => {
  const originalPlatform = process.platform

  beforeEach(() => {
    jest.clearAllMocks()
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  test('throws outside macOS', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    await expect(installOnMacFromFile('/tmp/LIVI.dmg')).rejects.toThrow('macOS only')
  })

  test('installs from dmg, relaunches and quits', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    await installOnMacFromFile('/tmp/LIVI.dmg')
    jest.runAllTimers()

    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'installing' })
    expect(installFromDmg).toHaveBeenCalledWith('/tmp/LIVI.dmg')
    expect(sendUpdateEvent).toHaveBeenCalledWith({ phase: 'relaunching' })
    expect(app.relaunch).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })
})
