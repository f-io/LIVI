import { buildTag, cmpSemver, human, parseSemver, sameNightlyBuild } from '../utils'

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

  test('sameNightlyBuild matches the short sha against the full commit', () => {
    expect(sameNightlyBuild('d7203c5', 'd7203c5d0f1e2a3b4c5d6e7f8091a2b3c4d5e6f7')).toBe(true)
    expect(sameNightlyBuild('D7203C5', 'd7203c5d0f1e2a3b4c5d6e7f8091a2b3c4d5e6f7')).toBe(true)
    expect(sameNightlyBuild(' d7203c5 ', 'd7203c5d0f1e2a3b4c5d6e7f8091a2b3c4d5e6f7')).toBe(true)
    expect(sameNightlyBuild('aaaaaaa', 'd7203c5d0f1e2a3b4c5d6e7f8091a2b3c4d5e6f7')).toBe(false)
  })

  test('sameNightlyBuild never calls an unstamped build current', () => {
    expect(sameNightlyBuild('dev', 'd7203c5d0f1e')).toBe(false)
    expect(sameNightlyBuild('', 'd7203c5d0f1e')).toBe(false)
    expect(sameNightlyBuild('d7203c5', '')).toBe(false)
    expect(sameNightlyBuild(undefined, undefined)).toBe(false)
  })

  test('buildTag joins run and sha, dropping what is missing', () => {
    expect(buildTag('123', '0f404b2')).toBe(' (#123 · 0f404b2)')
    expect(buildTag('', '0f404b2')).toBe(' (0f404b2)')
    expect(buildTag('123', '')).toBe(' (#123)')
    expect(buildTag('', '')).toBe('')
    expect(buildTag(undefined, undefined)).toBe('')
  })
})
