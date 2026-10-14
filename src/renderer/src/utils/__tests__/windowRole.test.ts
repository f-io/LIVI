import { getWindowRole } from '../windowRole'

function setSearch(qs: string): void {
  // jsdom locks window.location; mutate via the History API instead.
  window.history.replaceState({}, '', `/${qs}`)
}

describe('getWindowRole', () => {
  test('returns "main" with no role param', () => {
    setSearch('')
    expect(getWindowRole()).toBe('main')
  })

  test('returns "cluster" when role=cluster', () => {
    setSearch('?role=cluster')
    expect(getWindowRole()).toBe('cluster')
  })

  test('returns "aux" when role=aux', () => {
    setSearch('?role=aux')
    expect(getWindowRole()).toBe('aux')
  })

  test('unknown role falls back to "main"', () => {
    setSearch('?role=garbage')
    expect(getWindowRole()).toBe('main')
  })

  test('falls back to an empty search when window is undefined', () => {
    vi.stubGlobal('window', undefined)
    try {
      expect(getWindowRole()).toBe('main')
    } finally {
      vi.unstubAllGlobals()
    }
  })

  test('survives URLSearchParams throwing', () => {
    const origURLSearchParams = global.URLSearchParams
    ;(global as { URLSearchParams: unknown }).URLSearchParams = function () {
      throw new Error('boom')
    } as never
    try {
      expect(getWindowRole()).toBe('main')
    } finally {
      global.URLSearchParams = origURLSearchParams
    }
  })
})
