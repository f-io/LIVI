import { writeFileSync } from 'fs'
import { getMainWindow } from '@main/window/createWindow'
import { applyNullDeletes, pushSettingsToRenderer, sizesEqual } from '@main/utils'
import { configEvents, saveSettings, sendUpdateEvent, sendUpdateProgress } from '@main/ipc/utils'

jest.mock('fs', () => ({
  existsSync: jest.fn(() => false),
  writeFileSync: jest.fn()
}))

jest.mock('@main/config/paths', () => ({
  CONFIG_PATH: '/tmp/config.json'
}))

jest.mock('@shared/types', () => ({
  DEFAULT_BINDINGS: { play: 'Space' }
}))

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn()
}))

jest.mock('@main/utils', () => ({
  applyNullDeletes: jest.fn(),
  pushSettingsToRenderer: jest.fn(),
  sizesEqual: jest.fn(() => true)
}))

jest.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: jest.fn(),
  applyAspectRatioWindowed: jest.fn(),
  applyWindowedContentSize: jest.fn(),
  fitWindowToWorkArea: jest.fn()
}))

describe('ipc utils', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    configEvents.removeAllListeners('changed')
  })

  test('sendUpdateEvent forwards payload to renderer channel', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({ webContents: { send } })

    sendUpdateEvent({ phase: 'check' } as never)

    expect(send).toHaveBeenCalledWith('update:event', { phase: 'check' })
  })

  test('sendUpdateProgress forwards payload to renderer progress channel', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({ webContents: { send } })

    sendUpdateProgress({ phase: 'download', percent: 25 } as never)

    expect(send).toHaveBeenCalledWith('update:progress', { phase: 'download', percent: 25 })
  })

  test('saveSettings merges config and bindings, writes file and updates runtime state', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue(null)
    ;(sizesEqual as jest.Mock).mockReturnValue(true)

    const onChanged = jest.fn()
    configEvents.on('changed', onChanged)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: true, bindings: { prev: 'ArrowLeft' } }
    } as never

    const patch = { height: 600, bindings: { next: 'ArrowRight' }, language: 'de' } as never

    saveSettings(runtimeState, patch)

    expect(applyNullDeletes).toHaveBeenCalledWith(runtimeState.config, patch)
    expect(writeFileSync).toHaveBeenCalledWith(
      '/tmp/config.json',
      JSON.stringify(runtimeState.config, null, 2)
    )
    expect(pushSettingsToRenderer).toHaveBeenCalledWith(runtimeState)

    expect(runtimeState.config.height).toBe(600)
    expect(runtimeState.config.language).toBe('de')
    expect(runtimeState.config.bindings).toEqual({
      play: 'Space',
      prev: 'ArrowLeft',
      next: 'ArrowRight'
    })

    expect(onChanged).toHaveBeenCalled()
  })
})
