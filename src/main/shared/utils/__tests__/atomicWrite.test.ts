import { promises as fsp, renameSync, writeFileSync } from 'node:fs'
import { writeFileAtomic, writeFileAtomicAsync } from '../atomicWrite'

vi.mock('node:fs', () => ({
  renameSync: vi.fn(),
  writeFileSync: vi.fn(),
  promises: { rename: vi.fn(), writeFile: vi.fn() }
}))

const FILE = '/state/devices.json'
const TMP = '/state/devices.json.tmp'

describe('writeFileAtomic', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  test('writes the tmp file and renames it over the target', () => {
    writeFileAtomic(FILE, '[]')

    expect(writeFileSync).toHaveBeenCalledWith(TMP, '[]')
    expect(renameSync).toHaveBeenCalledWith(TMP, FILE)
  })

  test('carries the mode through to the tmp file', () => {
    writeFileAtomic(FILE, '{}', 0o600)

    expect(writeFileSync).toHaveBeenCalledWith(TMP, '{}', { mode: 0o600 })
  })

  test('a write that fails leaves the target as it was', () => {
    vi.mocked(writeFileSync).mockImplementation(() => {
      throw new Error('no space left on device')
    })

    expect(() => writeFileAtomic(FILE, '[]')).toThrow('no space left on device')
    expect(renameSync).not.toHaveBeenCalled()
  })
})

describe('writeFileAtomicAsync', () => {
  beforeEach(() => {
    vi.resetAllMocks()
  })

  test('writes the tmp file and renames it over the target', async () => {
    await writeFileAtomicAsync(FILE, '[]')

    expect(fsp.writeFile).toHaveBeenCalledWith(TMP, '[]')
    expect(fsp.rename).toHaveBeenCalledWith(TMP, FILE)
  })

  test('carries the mode through to the tmp file', async () => {
    await writeFileAtomicAsync(FILE, '{}', 0o600)

    expect(fsp.writeFile).toHaveBeenCalledWith(TMP, '{}', { mode: 0o600 })
  })

  test('a write that fails leaves the target as it was', async () => {
    vi.mocked(fsp.writeFile).mockRejectedValue(new Error('no space left on device'))

    await expect(writeFileAtomicAsync(FILE, '[]')).rejects.toThrow('no space left on device')
    expect(fsp.rename).not.toHaveBeenCalled()
  })
})
