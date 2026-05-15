type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

const existsSyncMock = jest.fn(() => true)
const readMediaFileMock = jest.fn(() => ({ ok: true, kind: 'media' }))
const readNavigationFileMock = jest.fn(() => ({ ok: true, kind: 'nav' }))

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: jest.fn()
}))

jest.mock('fs', () => ({ existsSync: (...a: unknown[]) => existsSyncMock(...a) }))
jest.mock('electron', () => ({ app: { getPath: () => '/tmp/livi' } }))
jest.mock('../../services/utils/readMediaFile', () => ({
  readMediaFile: (...a: unknown[]) => readMediaFileMock(...a)
}))
jest.mock('../../services/utils/readNavigationFile', () => ({
  readNavigationFile: (...a: unknown[]) => readNavigationFileMock(...a)
}))
jest.mock('../../services/constants', () => ({
  DEFAULT_MEDIA_DATA_RESPONSE: { __default: 'media' },
  DEFAULT_NAVIGATION_DATA_RESPONSE: { __default: 'nav' }
}))

import { registerDataIpc } from '../data'

beforeEach(() => {
  handlers.clear()
  existsSyncMock.mockReset().mockReturnValue(true)
  readMediaFileMock.mockReset().mockReturnValue({ ok: true, kind: 'media' })
  readNavigationFileMock.mockReset().mockReturnValue({ ok: true, kind: 'nav' })
  jest.spyOn(console, 'log').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('data ipc — projection-media-read', () => {
  test('reads the media file when it exists', async () => {
    registerDataIpc()
    const r = await handlers.get('projection-media-read')!(null)
    expect(r).toEqual({ ok: true, kind: 'media' })
    expect(readMediaFileMock).toHaveBeenCalled()
  })

  test('returns the default response when the file is missing', async () => {
    existsSyncMock.mockReturnValueOnce(false)
    registerDataIpc()
    const r = await handlers.get('projection-media-read')!(null)
    expect(r).toEqual({ __default: 'media' })
  })

  test('returns the default response when readMediaFile throws', async () => {
    readMediaFileMock.mockImplementationOnce(() => {
      throw new Error('parse error')
    })
    registerDataIpc()
    const r = await handlers.get('projection-media-read')!(null)
    expect(r).toEqual({ __default: 'media' })
  })
})

describe('data ipc — projection-navigation-read', () => {
  test('reads the navigation file when it exists', async () => {
    registerDataIpc()
    const r = await handlers.get('projection-navigation-read')!(null)
    expect(r).toEqual({ ok: true, kind: 'nav' })
  })

  test('returns the default response when the file is missing', async () => {
    existsSyncMock.mockReturnValueOnce(false)
    registerDataIpc()
    const r = await handlers.get('projection-navigation-read')!(null)
    expect(r).toEqual({ __default: 'nav' })
  })

  test('returns the default response when readNavigationFile throws', async () => {
    readNavigationFileMock.mockImplementationOnce(() => {
      throw new Error('IO')
    })
    registerDataIpc()
    const r = await handlers.get('projection-navigation-read')!(null)
    expect(r).toEqual({ __default: 'nav' })
  })
})
