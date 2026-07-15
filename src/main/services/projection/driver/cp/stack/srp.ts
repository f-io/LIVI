/**
 * srp — SRP-6a server (RFC 5054 3072-bit group, SHA-512), the HAP/AirPlay-2
 * pair-setup variant. Username is the literal "Pair-Setup", the password is the
 * setup code. Cleanroom, standard SRP-6a math over BigInt.
 */

import { createHash, randomBytes } from 'node:crypto'

// RFC 5054 3072-bit group.
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
const N_BYTES = 384 // 3072 bits

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

// k = H(N | PAD(g)) for SRP-6a
const k = toBigInt(sha512(toBuf(N), pad(g)))

export interface SrpServer {
  salt: Buffer
  /** Server public key B. */
  B: Buffer
  /** Verify the client's public key A and proof M1. Returns K + server proof M2 on success. */
  verify(A: Buffer, clientM1: Buffer): { ok: boolean; K?: Buffer; serverM2?: Buffer }
}

/**
 * Start an SRP-6a server session for the given username/password (setup code).
 */
export function srpStartServer(username: string, password: string): SrpServer {
  const salt = randomBytes(16)
  const I = Buffer.from(username, 'utf8')
  const p = Buffer.from(password, 'utf8')

  // x = H(salt | H(I | ":" | p)), verifier v = g^x mod N
  const x = toBigInt(sha512(salt, sha512(I, Buffer.from(':'), p)))
  const v = modPow(g, x, N)

  // b random, B = (k*v + g^b) mod N
  const b = toBigInt(randomBytes(32))
  const B = (k * v + modPow(g, b, N)) % N

  const verify = (Abuf: Buffer, clientM1: Buffer) => {
    const A = toBigInt(Abuf)
    if (A % N === 0n) return { ok: false }

    const u = toBigInt(sha512(pad(A), pad(B)))
    // S = (A * v^u)^b mod N
    const S = modPow((A * modPow(v, u, N)) % N, b, N)
    const K = sha512(toBuf(S))

    // M1 = H(H(N) XOR H(g) | H(I) | salt | A | B | K)
    const hN = sha512(toBuf(N))
    const hg = sha512(toBuf(g))
    const hXor = Buffer.alloc(hN.length)
    for (let i = 0; i < hN.length; i++) hXor[i] = hN[i] ^ hg[i]
    const expectM1 = sha512(hXor, sha512(I), salt, pad(A), pad(B), K)

    if (!expectM1.equals(clientM1)) return { ok: false }

    // M2 = H(A | M1 | K)
    const serverM2 = sha512(pad(A), clientM1, K)
    return { ok: true, K, serverM2 }
  }

  return { salt, B: pad(B), verify }
}
