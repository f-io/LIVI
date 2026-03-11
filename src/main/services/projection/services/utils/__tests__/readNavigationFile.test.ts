import fs from 'fs'
import { readNavigationFile } from '@main/services/projection/services/utils/readNavigationFile'
import { DEFAULT_NAVIGATION_DATA_RESPONSE } from '@main/services/projection/services/constants'

jest.mock('fs', () => ({
  readFileSync: jest.fn()
}))

describe('readNavigationFile', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('returns parsed persisted navigation payload', () => {
    const payload = {
      timestamp: '2026-01-01T00:00:00.000Z',
      payload: { metaType: 200, navi: { A: 1 }, rawUtf8: '', error: false }
    }
    ;(fs.readFileSync as jest.Mock).mockReturnValue(JSON.stringify(payload))

    expect(readNavigationFile('/tmp/nav.json')).toEqual(payload)
  })

  test('returns default response on read/parse failure', () => {
    ;(fs.readFileSync as jest.Mock).mockImplementation(() => {
      throw new Error('boom')
    })

    expect(readNavigationFile('/tmp/nav.json')).toEqual(DEFAULT_NAVIGATION_DATA_RESPONSE)
  })
})
