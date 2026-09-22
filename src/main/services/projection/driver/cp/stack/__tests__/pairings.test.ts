import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { getPairing, savePairing } from '../pairings'

vi.mock('node:fs', () => ({
  existsSync: vi.fn(() => false),
  mkdirSync: vi.fn(),
  readFileSync: vi.fn(),
  renameSync: vi.fn(),
  writeFileSync: vi.fn()
}))

const mockExists = vi.mocked(existsSync)
const mockRead = vi.mocked(readFileSync)
const mockWrite = vi.mocked(writeFileSync)
const mockRename = vi.mocked(renameSync)
const mockMkdir = vi.mocked(mkdirSync)

beforeEach(() => {
  vi.clearAllMocks()
  mockExists.mockReturnValue(false)
})

describe('savePairing', () => {
  test('writes the pairing keyed by identifier with restrictive mode', () => {
    savePairing('phone-1', Buffer.from('aabb', 'hex'))
    expect(mockMkdir).toHaveBeenCalledWith('/tmp/cp', { recursive: true })
    const [file, json, opts] = mockWrite.mock.calls[0]
    expect(file).toBe('/tmp/cp/pairings.json.tmp')
    expect(JSON.parse(json as string)).toEqual({ 'phone-1': 'aabb' })
    expect(opts).toEqual({ mode: 0o600 })
    expect(mockRename).toHaveBeenCalledWith('/tmp/cp/pairings.json.tmp', '/tmp/cp/pairings.json')
  })

  test('merges into existing pairings', () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue(JSON.stringify({ old: '1122' }))
    savePairing('new', Buffer.from('3344', 'hex'))
    expect(JSON.parse(mockWrite.mock.calls[0][1] as string)).toEqual({
      old: '1122',
      new: '3344'
    })
  })

  test('starts fresh when the store is unreadable', () => {
    mockExists.mockReturnValue(true)
    mockRead.mockImplementation(() => {
      throw new Error('EACCES')
    })
    savePairing('id', Buffer.from('ff', 'hex'))
    expect(JSON.parse(mockWrite.mock.calls[0][1] as string)).toEqual({ id: 'ff' })
  })

  test('warns instead of throwing when persisting fails', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    mockWrite.mockImplementation(() => {
      throw new Error('disk full')
    })
    expect(() => savePairing('id', Buffer.from('00', 'hex'))).not.toThrow()
    expect(warn).toHaveBeenCalledWith('[cpPairings] persist failed:', 'disk full')
    warn.mockRestore()
  })
})

describe('getPairing', () => {
  test('returns the stored key as a Buffer', () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue(JSON.stringify({ 'phone-1': 'deadbeef' }))
    expect(getPairing('phone-1')?.equals(Buffer.from('deadbeef', 'hex'))).toBe(true)
  })

  test('returns null for an unknown identifier', () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue(JSON.stringify({}))
    expect(getPairing('stranger')).toBeNull()
  })

  test('returns null when no store exists', () => {
    expect(getPairing('phone-1')).toBeNull()
  })

  test('returns null when the store is corrupt', () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue('{not json')
    expect(getPairing('phone-1')).toBeNull()
  })
})
