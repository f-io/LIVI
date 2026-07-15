type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerDataIpc } from '../data'

const ACTIVE_MEDIA = { timestamp: 't', payload: { type: 1, media: { MediaSongName: 'S' } } }
const ACTIVE_NAV = { timestamp: 't', payload: { metaType: 200, navi: null } }

const host = {
  readActiveMedia: vi.fn(() => ACTIVE_MEDIA),
  readActiveNav: vi.fn(() => ACTIVE_NAV)
}

beforeEach(async () => {
  handlers.clear()
  host.readActiveMedia.mockReset().mockReturnValue(ACTIVE_MEDIA)
  host.readActiveNav.mockReset().mockReturnValue(ACTIVE_NAV)
})
afterEach(async () => vi.restoreAllMocks())

describe('data ipc — projection-media-read', () => {
  test('returns the active media snapshot from the host', async () => {
    registerDataIpc(host as any)
    const r = await handlers.get('projection-media-read')!(null)
    expect(r).toEqual(ACTIVE_MEDIA)
    expect(host.readActiveMedia).toHaveBeenCalled()
  })
})

describe('data ipc — projection-navigation-read', () => {
  test('returns the active navigation snapshot from the host', async () => {
    registerDataIpc(host as any)
    const r = await handlers.get('projection-navigation-read')!(null)
    expect(r).toEqual(ACTIVE_NAV)
    expect(host.readActiveNav).toHaveBeenCalled()
  })
})
