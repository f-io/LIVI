import { createHash, randomBytes } from 'node:crypto'
import {
  chachaOpen,
  chachaSeal,
  ed25519Generate,
  ed25519Sign,
  ed25519Verify,
  hkdfSha512,
  nonceLabel
} from '../crypto'
import { loadOrCreateIdentity } from '../identity'
import { getPairing } from '../pairings'
import { PairSetup } from '../pairSetup'
import type { SrpServer } from '../srp'
import { decodeTlv8, encodeTlv8, type Tlv8Item } from '../tlv8'

vi.mock('node:fs', () => {
  const files = new Map<string, string>()
  return {
    existsSync: (p: string) => files.has(p),
    readFileSync: (p: string) => files.get(p) ?? '',
    writeFileSync: (p: string, data: string) => {
      files.set(p, data)
    },
    renameSync: (from: string, to: string) => {
      files.set(to, files.get(from) ?? '')
      files.delete(from)
    },
    mkdirSync: vi.fn()
  }
})

const N_HEX = `
FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74
020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437
4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED
EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05
98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB
9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B
E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF6955817183
995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507A33A
85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7AB
F5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864D8
7602733EC86A64521F2B18177B200CBBE117577A615D6C770988C0BAD946E208
E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF
`.replace(/\s+/g, '')

const N = BigInt(`0x${N_HEX}`)
const g = 5n
const N_BYTES = 384

function sha512(...parts: Buffer[]): Buffer {
  const h = createHash('sha512')
  for (const p of parts) h.update(p)
  return h.digest()
}

function toBuf(n: bigint): Buffer {
  let hex = n.toString(16)
  if (hex.length % 2) hex = `0${hex}`
  return Buffer.from(hex, 'hex')
}

function pad(n: bigint): Buffer {
  const b = toBuf(n)
  if (b.length >= N_BYTES) return b
  return Buffer.concat([Buffer.alloc(N_BYTES - b.length), b])
}

function toBigInt(b: Buffer): bigint {
  return b.length === 0 ? 0n : BigInt(`0x${b.toString('hex')}`)
}

function modPow(base: bigint, exp: bigint, mod: bigint): bigint {
  let result = 1n
  let b = base % mod
  let e = exp
  while (e > 0n) {
    if (e & 1n) result = (result * b) % mod
    e >>= 1n
    b = (b * b) % mod
  }
  return result
}

function runClient(
  Bbuf: Buffer,
  salt: Buffer,
  password: string
): { A: Buffer; M1: Buffer; K: Buffer } {
  let a = 0n
  let A = 0n
  do {
    a = toBigInt(randomBytes(32))
    A = modPow(g, a, N)
  } while (toBuf(A).length !== N_BYTES)

  const B = toBigInt(Bbuf)
  const u = toBigInt(sha512(pad(A), pad(B)))
  const k = toBigInt(sha512(toBuf(N), pad(g)))
  const I = Buffer.from('Pair-Setup', 'utf8')
  const p = Buffer.from(password, 'utf8')
  const x = toBigInt(sha512(salt, sha512(I, Buffer.from(':'), p)))
  const kgx = (k * modPow(g, x, N)) % N
  const base = (((B - kgx) % N) + N) % N
  const S = modPow(base, a + u * x, N)
  const K = sha512(toBuf(S))
  const hN = sha512(toBuf(N))
  const hg = sha512(toBuf(g))
  const hXor = Buffer.alloc(hN.length)
  for (let i = 0; i < hN.length; i++) hXor[i] = hN[i] ^ hg[i]
  const M1 = sha512(hXor, sha512(I), salt, pad(A), pad(B), K)
  return { A: pad(A), M1, K }
}

const TLV = {
  Identifier: 0x01,
  Salt: 0x02,
  PublicKey: 0x03,
  Proof: 0x04,
  EncryptedData: 0x05,
  State: 0x06,
  Error: 0x07,
  Signature: 0x0a
} as const

function msg(state: number, extra: Tlv8Item[] = []): Buffer {
  return encodeTlv8([{ type: TLV.State, value: Buffer.from([state]) }, ...extra])
}

function expectErr(res: Buffer, state: number): void {
  const tlv = decodeTlv8(res)
  expect(tlv.get(TLV.State)?.readUInt8(0)).toBe(state)
  expect(tlv.get(TLV.Error)?.readUInt8(0)).toBe(2)
}

function runToM4(ps: PairSetup, password = '3939'): { K: Buffer; m4: Map<number, Buffer> } {
  const m2 = decodeTlv8(ps.handle(msg(1)))
  const B = m2.get(TLV.PublicKey) as Buffer
  const salt = m2.get(TLV.Salt) as Buffer
  const client = runClient(B, salt, password)
  const m4 = decodeTlv8(
    ps.handle(
      msg(3, [
        { type: TLV.PublicKey, value: client.A },
        { type: TLV.Proof, value: client.M1 }
      ])
    )
  )
  return { K: client.K, m4 }
}

function m5Sub(K: Buffer, omit: number[] = [], badSig = false): Buffer {
  const ctrl = ed25519Generate()
  const ctrlId = Buffer.from('phone-1', 'utf8')
  const signKey = hkdfSha512(
    K,
    'Pair-Setup-Controller-Sign-Salt',
    'Pair-Setup-Controller-Sign-Info',
    32
  )
  const signer = badSig ? ed25519Generate() : ctrl
  const sig = ed25519Sign(signer.privRaw, Buffer.concat([signKey, ctrlId, ctrl.pubRaw]))
  const items: Tlv8Item[] = [
    { type: TLV.Identifier, value: ctrlId },
    { type: TLV.PublicKey, value: ctrl.pubRaw },
    { type: TLV.Signature, value: sig }
  ].filter((i) => !omit.includes(i.type))
  const encKey = hkdfSha512(K, 'Pair-Setup-Encrypt-Salt', 'Pair-Setup-Encrypt-Info', 32)
  return chachaSeal(encKey, nonceLabel('PS-Msg05'), encodeTlv8(items))
}

let logSpy: ReturnType<typeof vi.spyOn>
let warnSpy: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
  warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => {
  logSpy.mockRestore()
  warnSpy.mockRestore()
})

describe('PairSetup happy path', () => {
  test('M1 -> M2 starts SRP with a public key and salt', () => {
    const m2 = decodeTlv8(new PairSetup().handle(msg(1)))
    expect(m2.get(TLV.State)?.readUInt8(0)).toBe(2)
    expect(m2.get(TLV.PublicKey)?.length).toBe(384)
    expect(m2.get(TLV.Salt)?.length).toBe(16)
  })

  test('M3 -> M4 verifies the SRP proof and returns the server proof', () => {
    const ps = new PairSetup()
    const { K, m4 } = runToM4(ps)
    expect(m4.get(TLV.State)?.readUInt8(0)).toBe(4)
    expect(m4.get(TLV.Proof)?.length).toBe(64)
    expect(m4.get(TLV.Error)).toBeUndefined()
    expect(K.length).toBe(64)
    expect(ps.complete).toBe(false)
  })

  test('M5 -> M6 exchanges long-term keys and persists the controller', () => {
    const ps = new PairSetup()
    const { K } = runToM4(ps)
    const res = decodeTlv8(ps.handle(msg(5, [{ type: TLV.EncryptedData, value: m5Sub(K) }])))
    expect(res.get(TLV.State)?.readUInt8(0)).toBe(6)
    expect(ps.complete).toBe(true)
    expect(getPairing('phone-1')).not.toBeNull()

    const encKey = hkdfSha512(K, 'Pair-Setup-Encrypt-Salt', 'Pair-Setup-Encrypt-Info', 32)
    const sub = decodeTlv8(
      chachaOpen(encKey, nonceLabel('PS-Msg06'), res.get(TLV.EncryptedData) as Buffer)
    )
    const id = loadOrCreateIdentity()
    expect(sub.get(TLV.Identifier)?.toString('utf8')).toBe(id.pairingId)
    expect(sub.get(TLV.PublicKey)?.equals(id.pubRaw)).toBe(true)
    const accSignKey = hkdfSha512(
      K,
      'Pair-Setup-Accessory-Sign-Salt',
      'Pair-Setup-Accessory-Sign-Info',
      32
    )
    const signData = Buffer.concat([accSignKey, Buffer.from(id.pairingId, 'utf8'), id.pubRaw])
    expect(ed25519Verify(id.pubRaw, signData, sub.get(TLV.Signature) as Buffer)).toBe(true)
  })
})

describe('PairSetup state handling', () => {
  test('a body without a state answers error state 0', () => {
    expectErr(new PairSetup().handle(encodeTlv8([])), 0)
  })

  test('an unhandled state echoes back as an error', () => {
    expectErr(new PairSetup().handle(msg(9)), 9)
  })

  test('a throwing step is caught and answered as an error', () => {
    const ps = new PairSetup()
    runToM4(ps)
    const res = ps.handle(msg(5, [{ type: TLV.EncryptedData, value: Buffer.alloc(40, 1) }]))
    expectErr(res, 5)
    expect(warnSpy).toHaveBeenCalledWith('[pairSetup] error:', expect.any(String))
  })
})

describe('PairSetup M4 rejections', () => {
  test('rejects M3 before M1', () => {
    const ps = new PairSetup()
    const res = ps.handle(
      msg(3, [
        { type: TLV.PublicKey, value: Buffer.alloc(384, 1) },
        { type: TLV.Proof, value: Buffer.alloc(64) }
      ])
    )
    expectErr(res, 4)
  })

  test('rejects M3 without a public key or proof', () => {
    const ps = new PairSetup()
    ps.handle(msg(1))
    expectErr(ps.handle(msg(3, [{ type: TLV.Proof, value: Buffer.alloc(64) }])), 4)
    expectErr(ps.handle(msg(3, [{ type: TLV.PublicKey, value: Buffer.alloc(384, 1) }])), 4)
  })

  test('rejects a client that used the wrong setup code', () => {
    const ps = new PairSetup()
    const { m4 } = runToM4(ps, '0000')
    expect(m4.get(TLV.Error)?.readUInt8(0)).toBe(2)
    expect(warnSpy).toHaveBeenCalledWith('[pairSetup] SRP verify failed')
  })

  test('rejects an SRP result without session key material', () => {
    const ps = new PairSetup()
    const fakeSrp: SrpServer = {
      salt: Buffer.alloc(16),
      B: Buffer.alloc(384, 1),
      verify: () => ({ ok: true })
    }
    ;(ps as unknown as { srp: SrpServer }).srp = fakeSrp
    const req = msg(3, [
      { type: TLV.PublicKey, value: Buffer.alloc(384, 1) },
      { type: TLV.Proof, value: Buffer.alloc(64) }
    ])
    expectErr(ps.handle(req), 4)
    fakeSrp.verify = () => ({ ok: true, K: Buffer.alloc(64) })
    ;(ps as unknown as { srp: SrpServer }).srp = fakeSrp
    expectErr(ps.handle(req), 4)
  })
})

describe('PairSetup M6 rejections', () => {
  test('rejects M5 before the SRP exchange', () => {
    const ps = new PairSetup()
    expectErr(ps.handle(msg(5, [{ type: TLV.EncryptedData, value: Buffer.alloc(20) }])), 6)
  })

  test('rejects M5 without encrypted data', () => {
    const ps = new PairSetup()
    runToM4(ps)
    expectErr(ps.handle(msg(5)), 6)
  })

  test('rejects a sub-TLV missing identifier, key or signature', () => {
    for (const omit of [TLV.Identifier, TLV.PublicKey, TLV.Signature]) {
      const ps = new PairSetup()
      const { K } = runToM4(ps)
      const res = ps.handle(msg(5, [{ type: TLV.EncryptedData, value: m5Sub(K, [omit]) }]))
      expectErr(res, 6)
      expect(ps.complete).toBe(false)
    }
  })

  test('rejects an invalid controller signature', () => {
    const ps = new PairSetup()
    const { K } = runToM4(ps)
    const res = ps.handle(msg(5, [{ type: TLV.EncryptedData, value: m5Sub(K, [], true) }]))
    expectErr(res, 6)
    expect(warnSpy).toHaveBeenCalledWith('[pairSetup] controller signature invalid')
  })
})
