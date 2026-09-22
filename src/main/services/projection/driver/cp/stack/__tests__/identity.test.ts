import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs'
import { ed25519Sign, ed25519Verify } from '../crypto'

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
const mockMkdir = vi.mocked(mkdirSync)

async function freshLoad(): Promise<typeof import('../identity').loadOrCreateIdentity> {
  vi.resetModules()
  return (await import('../identity')).loadOrCreateIdentity
}

beforeEach(() => {
  vi.clearAllMocks()
  mockExists.mockReturnValue(false)
})

describe('loadOrCreateIdentity', () => {
  test('generates and persists a new identity when none exists', async () => {
    const load = await freshLoad()
    const id = load()
    expect(id.privRaw.length).toBe(32)
    expect(id.pubRaw.length).toBe(32)
    expect(id.pkHex).toBe(id.pubRaw.toString('hex'))
    expect(id.pairingId).toMatch(/^[0-9a-f-]{36}$/)
    expect(mockMkdir).toHaveBeenCalledWith('/tmp/cp', { recursive: true })
    const [file, json, opts] = mockWrite.mock.calls[0]
    expect(file).toBe('/tmp/cp/identity.json.tmp')
    expect(opts).toEqual({ mode: 0o600 })
    expect(vi.mocked(renameSync)).toHaveBeenCalledWith(
      '/tmp/cp/identity.json.tmp',
      '/tmp/cp/identity.json'
    )
    expect(JSON.parse(json as string)).toEqual({
      priv: id.privRaw.toString('hex'),
      pub: id.pubRaw.toString('hex'),
      pi: id.pairingId
    })
  })

  test('the generated key pair signs and verifies', async () => {
    const load = await freshLoad()
    const id = load()
    const sig = ed25519Sign(id.privRaw, Buffer.from('proof'))
    expect(ed25519Verify(id.pubRaw, Buffer.from('proof'), sig)).toBe(true)
  })

  test('returns the cached identity on subsequent calls', async () => {
    const load = await freshLoad()
    const first = load()
    mockExists.mockImplementation(() => {
      throw new Error('must not touch fs again')
    })
    expect(load()).toBe(first)
  })

  test('loads a persisted identity from disk', async () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue(
      JSON.stringify({ priv: '11'.repeat(32), pub: '22'.repeat(32), pi: 'stable-id' })
    )
    const load = await freshLoad()
    const id = load()
    expect(id.privRaw.equals(Buffer.alloc(32, 0x11))).toBe(true)
    expect(id.pubRaw.equals(Buffer.alloc(32, 0x22))).toBe(true)
    expect(id.pairingId).toBe('stable-id')
    expect(id.pkHex).toBe('22'.repeat(32))
    expect(mockWrite).not.toHaveBeenCalled()
  })

  test('regenerates when the persisted file is corrupt', async () => {
    mockExists.mockReturnValue(true)
    mockRead.mockReturnValue('{broken')
    const load = await freshLoad()
    const id = load()
    expect(id.privRaw.length).toBe(32)
    expect(mockWrite).toHaveBeenCalled()
  })

  test('still returns an identity when persisting fails', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    mockWrite.mockImplementation(() => {
      throw new Error('read-only fs')
    })
    const load = await freshLoad()
    const id = load()
    expect(id.pubRaw.length).toBe(32)
    expect(warn).toHaveBeenCalledWith('[cpIdentity] could not persist identity:', 'read-only fs')
    warn.mockRestore()
  })
})
