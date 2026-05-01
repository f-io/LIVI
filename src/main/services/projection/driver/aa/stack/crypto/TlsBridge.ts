/**
 * In-memory TLS shim for the Android Auto protocol.
 *
 * The AA protocol wraps TLS bytes inside frames rather than using socket-level TLS:
 *   - During handshake: TLS bytes travel in SSL_HANDSHAKE frames (msgId 0x0003)
 *   - After handshake:  each frame payload IS a TLS record (header + AES-GCM ciphertext)
 *
 * This class bridges that: it presents a Duplex stream to Node.js TLSSocket
 * while routing the raw TLS bytes through the AA frame layer.
 *
 * Roles:
 *   Phone = TLS server  (setUseClientMode(false) in APK)
 *   HU    = TLS client  (us)
 */

import { Duplex } from 'node:stream'
import * as tls from 'node:tls'

export type TlsSendFn = (tlsBytes: Buffer) => void

export class TlsBridge extends Duplex {
  constructor(private readonly _send: TlsSendFn) {
    super()
  }

  // Called by TLSSocket when it has bytes to transmit (wraps outgoing TLS records)
  _write(chunk: Buffer, _enc: BufferEncoding, cb: (err?: Error | null) => void): void {
    this._send(chunk)
    cb()
  }

  // Not used — we push data in via injectBytes()
  _read(_size: number): void {}

  /**
   * Feed raw TLS bytes received from the phone into the TLS engine.
   */
  injectBytes(data: Buffer): void {
    this.push(data)
  }
}

/**
 * Create a TLS client session piped through an AA TlsBridge.
 */
export function createTlsClient(
  certPem: string,
  keyPem: string,
  send: TlsSendFn
): { tlsSocket: tls.TLSSocket; bridge: TlsBridge } {
  const bridge = new TlsBridge(send)

  const ctx = tls.createSecureContext({
    cert: certPem, // Standard Google Automotive Link-signed HU cert (see cert.ts)
    key: keyPem // Android Auto requires the HU to present this specific cert
  })

  const tlsSocket = tls.connect({
    socket: bridge as unknown as import('net').Socket,
    secureContext: ctx,
    rejectUnauthorized: false,
    checkServerIdentity: () => undefined,
    minVersion: 'TLSv1.2' as tls.SecureVersion,
    maxVersion: 'TLSv1.2' as tls.SecureVersion
  })

  return { tlsSocket, bridge }
}
