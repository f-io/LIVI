import { cmpSemver, human, parseSemver } from '../utils'

describe('softwareUpdate utils', () => {
  test('parseSemver parses x.y.z only', () => {
    expect(parseSemver('1.2.3')).toEqual([1, 2, 3])
    expect(parseSemver(' 1.2.3 ')).toEqual([1, 2, 3])
    expect(parseSemver('1.2')).toBeNull()
    expect(parseSemver(undefined)).toBeNull()
  })

  test('cmpSemver compares versions correctly', () => {
    expect(cmpSemver([1, 2, 3], [1, 2, 3])).toBe(0)
    expect(cmpSemver([1, 2, 4], [1, 2, 3])).toBe(1)
    expect(cmpSemver([1, 1, 9], [1, 2, 0])).toBe(-1)
  })

  test('human formats bytes as KB/MB', () => {
    expect(human(2048)).toBe('2 KB')
    expect(human(2 * 1024 * 1024)).toBe('2.0 MB')
  })
})
