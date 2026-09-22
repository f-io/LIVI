import { setupAppIdentity } from '@main/app/init'
import { app } from 'electron'

vi.mock('electron', () => ({
  app: { commandLine: { appendSwitch: vi.fn() } }
}))

describe('setupAppIdentity', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  test('keeps the renderer running when it loses focus', () => {
    setupAppIdentity()

    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('disable-renderer-backgrounding')
  })
})
