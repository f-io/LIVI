import { electronApp } from '@electron-toolkit/utils'
import { setupAppIdentity } from '@main/app/init'

jest.mock('@electron-toolkit/utils', () => ({
  electronApp: {
    setAppUserModelId: jest.fn()
  }
}))

describe('setupAppIdentity', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('sets expected app user model id', () => {
    setupAppIdentity()

    expect(electronApp.setAppUserModelId).toHaveBeenCalledWith('com.livi.app')
  })
})
