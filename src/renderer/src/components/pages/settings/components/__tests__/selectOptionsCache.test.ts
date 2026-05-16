import {
  _resetSelectOptionsCache,
  getCachedOptions,
  resolveOptions,
  setCachedOptions
} from '../selectOptionsCache'

const baseNode = {
  path: 'someField',
  options: [{ value: 0, label: 'Static' }]
} as const

beforeEach(() => {
  _resetSelectOptionsCache()
})

describe('selectOptionsCache', () => {
  test('returns static options when no loadOptions is provided', async () => {
    const got = await resolveOptions(baseNode)
    expect(got).toEqual([{ value: 0, label: 'Static' }])
  })

  test('returns cached options on second call', async () => {
    const load = jest.fn(async () => [{ value: 'a', label: 'A' }])
    const node = { path: 'p', options: [], loadOptions: load }
    await resolveOptions(node)
    await resolveOptions(node)
    expect(load).toHaveBeenCalledTimes(1)
  })

  test('force: true bypasses the cache', async () => {
    const load = jest
      .fn()
      .mockResolvedValueOnce([{ value: 'old', label: 'Old' }])
      .mockResolvedValueOnce([{ value: 'new', label: 'New' }])
    const node = { path: 'p', options: [], loadOptions: load }
    const first = await resolveOptions(node)
    const second = await resolveOptions(node, { force: true })
    expect(first).toEqual([{ value: 'old', label: 'Old' }])
    expect(second).toEqual([{ value: 'new', label: 'New' }])
    expect(load).toHaveBeenCalledTimes(2)
  })

  test('inflight de-dup: concurrent resolves only call loadOptions once', async () => {
    let resolveLoad!: (v: Array<{ value: string; label: string }>) => void
    const load = jest.fn(
      () =>
        new Promise<Array<{ value: string; label: string }>>((res) => {
          resolveLoad = res
        })
    )
    const node = { path: 'p', options: [], loadOptions: load }
    const a = resolveOptions(node)
    const b = resolveOptions(node)
    resolveLoad([{ value: 'a', label: 'A' }])
    const [r1, r2] = await Promise.all([a, b])
    expect(r1).toEqual([{ value: 'a', label: 'A' }])
    expect(r2).toEqual([{ value: 'a', label: 'A' }])
    expect(load).toHaveBeenCalledTimes(1)
  })

  test('loadOptions rejection falls back to static options', async () => {
    const load = jest.fn(async () => {
      throw new Error('ipc down')
    })
    const node = { path: 'p', options: [{ value: 1, label: 'Fallback' }], loadOptions: load }
    const got = await resolveOptions(node)
    expect(got).toEqual([{ value: 1, label: 'Fallback' }])
  })

  test('setCachedOptions / getCachedOptions round-trip', () => {
    setCachedOptions({ path: 'p2' }, [{ value: 'x', label: 'X' }])
    expect(getCachedOptions({ path: 'p2' })).toEqual([{ value: 'x', label: 'X' }])
  })

  test('cache is per-path', async () => {
    const loadA = jest.fn(async () => [{ value: 'a', label: 'A' }])
    const loadB = jest.fn(async () => [{ value: 'b', label: 'B' }])
    await resolveOptions({ path: 'a', options: [], loadOptions: loadA })
    await resolveOptions({ path: 'b', options: [], loadOptions: loadB })
    expect(getCachedOptions({ path: 'a' })).toEqual([{ value: 'a', label: 'A' }])
    expect(getCachedOptions({ path: 'b' })).toEqual([{ value: 'b', label: 'B' }])
  })
})
