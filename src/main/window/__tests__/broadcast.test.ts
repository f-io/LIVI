const getMainWindowMock = jest.fn()
const getSecondaryWindowMock = jest.fn()

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: () => getMainWindowMock()
}))

jest.mock('@main/window/secondaryWindows', () => ({
  getSecondaryWindow: (role: string) => getSecondaryWindowMock(role)
}))

import {
  broadcastToRenderers,
  broadcastToSecondaryRenderers,
  getAllRendererWebContents,
  getSecondaryRendererWebContents
} from '../broadcast'

function fakeWin(over: { destroyed?: boolean; sendThrows?: boolean } = {}) {
  return {
    isDestroyed: jest.fn(() => over.destroyed ?? false),
    webContents: {
      send: jest.fn(() => {
        if (over.sendThrows) throw new Error('detached')
      })
    }
  }
}

beforeEach(() => {
  getMainWindowMock.mockReset()
  getSecondaryWindowMock.mockReset()
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

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

  test('appends dash + aux when they are alive', () => {
    const main = fakeWin()
    const dash = fakeWin()
    const aux = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) =>
      role === 'dash' ? dash : role === 'aux' ? aux : null
    )
    expect(getAllRendererWebContents()).toEqual([
      main.webContents,
      dash.webContents,
      aux.webContents
    ])
  })

  test('skips destroyed secondary windows', () => {
    getMainWindowMock.mockReturnValue(null)
    getSecondaryWindowMock.mockImplementation((role: string) =>
      role === 'dash' ? fakeWin({ destroyed: true }) : null
    )
    expect(getAllRendererWebContents()).toEqual([])
  })
})

describe('getSecondaryRendererWebContents', () => {
  test('returns only the secondary webContents (no main)', () => {
    const main = fakeWin()
    const dash = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'dash' ? dash : null))
    expect(getSecondaryRendererWebContents()).toEqual([dash.webContents])
  })

  test('empty list when neither secondary is open', () => {
    getSecondaryWindowMock.mockReturnValue(null)
    expect(getSecondaryRendererWebContents()).toEqual([])
  })
})

describe('broadcastToRenderers', () => {
  test('forwards channel + args to every alive renderer', () => {
    const main = fakeWin()
    const dash = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'dash' ? dash : null))

    broadcastToRenderers('foo:bar', { p: 1 })
    expect(main.webContents.send).toHaveBeenCalledWith('foo:bar', { p: 1 })
    expect(dash.webContents.send).toHaveBeenCalledWith('foo:bar', { p: 1 })
  })

  test('a thrown send is swallowed and warned, others still receive', () => {
    const broken = fakeWin({ sendThrows: true })
    const ok = fakeWin()
    getMainWindowMock.mockReturnValue(broken)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'dash' ? ok : null))

    expect(() => broadcastToRenderers('x')).not.toThrow()
    expect(ok.webContents.send).toHaveBeenCalled()
  })
})

describe('broadcastToSecondaryRenderers', () => {
  test('skips the main window', () => {
    const main = fakeWin()
    const aux = fakeWin()
    getMainWindowMock.mockReturnValue(main)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'aux' ? aux : null))

    broadcastToSecondaryRenderers('foo:bar', 1, 2)
    expect(main.webContents.send).not.toHaveBeenCalled()
    expect(aux.webContents.send).toHaveBeenCalledWith('foo:bar', 1, 2)
  })

  test('thrown send is swallowed and warned', () => {
    const broken = fakeWin({ sendThrows: true })
    getMainWindowMock.mockReturnValue(null)
    getSecondaryWindowMock.mockImplementation((role: string) => (role === 'dash' ? broken : null))
    expect(() => broadcastToSecondaryRenderers('x')).not.toThrow()
  })

  test('no-op when no secondary windows are open', () => {
    getSecondaryWindowMock.mockReturnValue(null)
    expect(() => broadcastToSecondaryRenderers('x')).not.toThrow()
  })
})
