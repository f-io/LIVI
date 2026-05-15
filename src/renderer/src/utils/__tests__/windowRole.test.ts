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

  test('returns "dash" when role=dash', () => {
    setSearch('?role=dash')
    expect(getWindowRole()).toBe('dash')
  })

  test('returns "aux" when role=aux', () => {
    setSearch('?role=aux')
    expect(getWindowRole()).toBe('aux')
  })

  test('unknown role falls back to "main"', () => {
    setSearch('?role=garbage')
    expect(getWindowRole()).toBe('main')
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
