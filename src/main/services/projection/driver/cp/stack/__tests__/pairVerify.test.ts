import type { KeyObject } from 'node:crypto'
import {
  chachaOpen,
  chachaSeal,
  ed25519Generate,
  ed25519Sign,
  ed25519Verify,
  hkdfSha512,
  nonceLabel,
  x25519Generate,
  x25519Shared
} from '../crypto'
import { loadOrCreateIdentity } from '../identity'
import { savePairing } from '../pairings'
import { PairVerify } from '../pairVerify'
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

const TLV = {
  Identifier: 0x01,
  PublicKey: 0x03,
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

interface Phone {
  priv: KeyObject
  pubRaw: Buffer
  serverPub: Buffer
  shared: Buffer
  encKey: Buffer
}

function runM1(pv: PairVerify): { phone: Phone; m2: Map<number, Buffer> } {
  const eph = x25519Generate()
  const m2 = decodeTlv8(pv.handle(msg(1, [{ type: TLV.PublicKey, value: eph.pubRaw }])))
  const serverPub = m2.get(TLV.PublicKey) as Buffer
  const shared = x25519Shared(eph.priv, serverPub)
  const encKey = hkdfSha512(shared, 'Pair-Verify-Encrypt-Salt', 'Pair-Verify-Encrypt-Info', 32)
  return { phone: { priv: eph.priv, pubRaw: eph.pubRaw, serverPub, shared, encKey }, m2 }
}

function m3Body(phone: Phone, ctrlId: string, ltsk: Buffer, omit: number[] = []): Buffer {
  const sig = ed25519Sign(
    ltsk,
    Buffer.concat([phone.pubRaw, Buffer.from(ctrlId, 'utf8'), phone.serverPub])
  )
  const items: Tlv8Item[] = [
    { type: TLV.Identifier, value: Buffer.from(ctrlId, 'utf8') },
    { type: TLV.Signature, value: sig }
  ].filter((i) => !omit.includes(i.type))
  const sealed = chachaSeal(phone.encKey, nonceLabel('PV-Msg03'), encodeTlv8(items))
  return msg(3, [{ type: TLV.EncryptedData, value: sealed }])
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

describe('PairVerify M2', () => {
  test('answers M1 with its ephemeral key and a verifiable signed identity', () => {
    const pv = new PairVerify()
    const { phone, m2 } = runM1(pv)
    expect(m2.get(TLV.State)?.readUInt8(0)).toBe(2)
    expect(phone.serverPub.length).toBe(32)

    const sub = decodeTlv8(
      chachaOpen(phone.encKey, nonceLabel('PV-Msg02'), m2.get(TLV.EncryptedData) as Buffer)
    )
    const id = loadOrCreateIdentity()
    const accId = sub.get(TLV.Identifier) as Buffer
    expect(accId.toString('utf8')).toBe(id.pairingId)
    const signData = Buffer.concat([phone.serverPub, accId, phone.pubRaw])
    expect(ed25519Verify(id.pubRaw, signData, sub.get(TLV.Signature) as Buffer)).toBe(true)
    expect(pv.sharedSecret?.equals(phone.shared)).toBe(true)
    expect(pv.verified).toBe(false)
  })

  test('rejects M1 without a public key', () => {
    expectErr(new PairVerify().handle(msg(1)), 2)
  })
})

describe('PairVerify M4', () => {
  test('verifies a known controller and derives the control keys', () => {
    const ctrl = ed25519Generate()
    savePairing('ctrl-1', ctrl.pubRaw)
    const pv = new PairVerify()
    const { phone } = runM1(pv)
    const res = decodeTlv8(pv.handle(m3Body(phone, 'ctrl-1', ctrl.privRaw)))
    expect(res.get(TLV.State)?.readUInt8(0)).toBe(4)
    expect(res.get(TLV.Error)).toBeUndefined()
    expect(pv.verified).toBe(true)
    expect(pv.controllerId).toBe('ctrl-1')
    const keys = pv.controlKeys
    expect(
      keys?.readKey.equals(
        hkdfSha512(phone.shared, 'Control-Salt', 'Control-Write-Encryption-Key', 32)
      )
    ).toBe(true)
    expect(
      keys?.writeKey.equals(
        hkdfSha512(phone.shared, 'Control-Salt', 'Control-Read-Encryption-Key', 32)
      )
    ).toBe(true)
  })

  test('rejects M3 before M1', () => {
    const pv = new PairVerify()
    expectErr(pv.handle(msg(3, [{ type: TLV.EncryptedData, value: Buffer.alloc(20) }])), 4)
    expect(pv.verified).toBe(false)
    expect(pv.controllerId).toBeNull()
    expect(pv.controlKeys).toBeNull()
    expect(pv.sharedSecret).toBeNull()
  })

  test('rejects M3 without encrypted data', () => {
    const pv = new PairVerify()
    runM1(pv)
    expectErr(pv.handle(msg(3)), 4)
  })

  test('answers an error when the encrypted data does not authenticate', () => {
    const pv = new PairVerify()
    runM1(pv)
    expectErr(pv.handle(msg(3, [{ type: TLV.EncryptedData, value: Buffer.alloc(24, 7) }])), 3)
    expect(warnSpy).toHaveBeenCalledWith('[pairVerify] error:', expect.any(String))
  })

  test('rejects a sub-TLV missing the identifier or signature', () => {
    const ctrl = ed25519Generate()
    savePairing('ctrl-2', ctrl.pubRaw)
    for (const omit of [TLV.Identifier, TLV.Signature]) {
      const pv = new PairVerify()
      const { phone } = runM1(pv)
      expectErr(pv.handle(m3Body(phone, 'ctrl-2', ctrl.privRaw, [omit])), 4)
      expect(pv.verified).toBe(false)
    }
  })

  test('rejects an unknown controller', () => {
    const ctrl = ed25519Generate()
    const pv = new PairVerify()
    const { phone } = runM1(pv)
    expectErr(pv.handle(m3Body(phone, 'stranger', ctrl.privRaw)), 4)
    expect(warnSpy).toHaveBeenCalledWith('[pairVerify] unknown controller stranger')
  })

  test('rejects an invalid controller signature', () => {
    const ctrl = ed25519Generate()
    const other = ed25519Generate()
    savePairing('ctrl-3', ctrl.pubRaw)
    const pv = new PairVerify()
    const { phone } = runM1(pv)
    expectErr(pv.handle(m3Body(phone, 'ctrl-3', other.privRaw)), 4)
    expect(warnSpy).toHaveBeenCalledWith('[pairVerify] controller signature invalid')
    expect(pv.verified).toBe(false)
  })
})

describe('PairVerify state handling', () => {
  test('a body without a state answers error state 0', () => {
    expectErr(new PairVerify().handle(encodeTlv8([])), 0)
  })

  test('an unhandled state echoes back as an error', () => {
    expectErr(new PairVerify().handle(msg(9)), 9)
  })
})
