import { isHttpUrlInput, normalizeHttpUrl } from '@shared/utils/httpUrl'

describe('normalizeHttpUrl', () => {
  test('reads a bare host as http', () => {
    expect(normalizeHttpUrl('10.0.0.9')).toBe('http://10.0.0.9/')
    expect(normalizeHttpUrl('this.local')).toBe('http://this.local/')
    expect(normalizeHttpUrl('localhost')).toBe('http://localhost/')
    expect(normalizeHttpUrl('127.0.0.1:8080')).toBe('http://127.0.0.1:8080/')
  })

  test('keeps a scheme that is given', () => {
    expect(normalizeHttpUrl('https://this.local')).toBe('https://this.local/')
    expect(normalizeHttpUrl('http://10.0.0.9/live')).toBe('http://10.0.0.9/live')
  })

  test('a scheme lets a single label through', () => {
    expect(normalizeHttpUrl('http://esp')).toBe('http://esp/')
  })

  test('refuses a single label without a scheme', () => {
    expect(normalizeHttpUrl('h')).toBeNull()
    expect(normalizeHttpUrl('esp')).toBeNull()
  })

  test('refuses what is no http address', () => {
    expect(normalizeHttpUrl('')).toBeNull()
    expect(normalizeHttpUrl('   ')).toBeNull()
    expect(normalizeHttpUrl('ftp://host.local')).toBeNull()
    expect(normalizeHttpUrl('http://')).toBeNull()
    expect(normalizeHttpUrl('not a url')).toBeNull()
  })

  test('trims what surrounds the address', () => {
    expect(normalizeHttpUrl('  10.0.0.9  ')).toBe('http://10.0.0.9/')
  })
})

describe('isHttpUrlInput', () => {
  test('accepts an empty field and any address that normalizes', () => {
    expect(isHttpUrlInput('')).toBe(true)
    expect(isHttpUrlInput('   ')).toBe(true)
    expect(isHttpUrlInput('10.0.0.9')).toBe(true)
    expect(isHttpUrlInput('https://f-io.dev')).toBe(true)
  })

  test('refuses a half-typed address and anything that is no string', () => {
    expect(isHttpUrlInput('h')).toBe(false)
    expect(isHttpUrlInput('ftp://x.y')).toBe(false)
    expect(isHttpUrlInput(42)).toBe(false)
    expect(isHttpUrlInput(undefined)).toBe(false)
  })
})
