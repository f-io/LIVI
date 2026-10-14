const getMainWindowMock = vi.fn()
const getSecondaryWindowMock = vi.fn()

vi.mock('@main/window/createWindow', () => ({
  getMainWindow: () => getMainWindowMock()
}))

vi.mock('@main/window/secondaryWindows', () => ({
  getSecondaryWindow: (role: string) => getSecondaryWindowMock(role)
}))

import { broadcastToRenderers, getAllRendererWebContents } from '../broadcast'

function fakeWin(over: { destroyed?: boolean; sendThrows?: boolean } = {}) {
  return {
    isDestroyed: vi.fn(() => over.destroyed ?? false),
    webContents: {
      send: vi.fn(function () {
        if (over.sendThrows) throw new Error('detached')
      })
    }
  }
}

beforeEach(() => {
  getMainWindowMock.mockReset()
  getSecondaryWindowMock.mockReset()
  vi.spyOn(console, 'warn').mockImplementation(function () {})
})
afterEach(() => vi.restoreAllMocks())

describe('getAllRendererWebContents', () => {
  test('returns the main webContents when alive', () => {
    const main = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    expect(getAllRendererWebContents()).toEqual([main.webContents])
  })

  test('skips destroyed main window', () => {
    getMainWindowMock.mockReturnValue(fakeWin({ destroyed: true }))
    expect(getAllRendererWebContents()).toEqual([])
  })

  test('skips when getMainWindow returns null', () => {
    getMainWindowMock.mockReturnValue(null)
    expect(getAllRendererWebContents()).toEqual([])
  })

  test('appends cluster + aux when they are alive', () => {
    const main = fakeWin()
    const cluster = fakeWin()
    const aux = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) =>
      role === 'cluster' ? cluster : role === 'aux' ? aux : null
    )
    expect(getAllRendererWebContents()).toEqual([
      main.webContents,
      cluster.webContents,
      aux.webContents
    ])
  })

  test('skips destroyed secondary windows', () => {
    getMainWindowMock.mockReturnValue(null)
    getSecondaryWindowMock.mockImplementation((role: string) =>
      role === 'cluster' ? fakeWin({ destroyed: true }) : null
    )
    expect(getAllRendererWebContents()).toEqual([])
  })
})

describe('broadcastToRenderers', () => {
  test('forwards channel + args to every alive renderer', () => {
    const main = fakeWin()
    const cluster = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) =>
      role === 'cluster' ? cluster : null
    )

    broadcastToRenderers('foo:bar', { p: 1 })
    expect(main.webContents.send).toHaveBeenCalledWith('foo:bar', { p: 1 })
    expect(cluster.webContents.send).toHaveBeenCalledWith('foo:bar', { p: 1 })
  })

  test('a thrown send is swallowed and warned, others still receive', () => {
    const broken = fakeWin({ sendThrows: true })
    const ok = fakeWin()
    getMainWindowMock.mockReturnValue(broken)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'cluster' ? ok : null))

    expect(() => broadcastToRenderers('x')).not.toThrow()
    expect(ok.webContents.send).toHaveBeenCalled()
  })
})
