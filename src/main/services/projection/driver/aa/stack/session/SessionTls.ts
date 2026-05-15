import type * as tls from 'node:tls'
import { DEBUG, TRACE } from '@main/constants'
import { CH, CTRL_MSG, FRAME_FLAGS } from '../constants.js'
import { HU_CERT_PEM, HU_KEY_PEM } from '../crypto/cert.js'
import { createTlsClient, TlsBridge } from '../crypto/TlsBridge.js'
import { encodeFrame } from '../frame/codec.js'

const FRAME_CHANNELS = new Set<number>([
  CH.VIDEO,
  CH.CLUSTER_VIDEO,
  CH.MEDIA_AUDIO,
  CH.SPEECH_AUDIO,
  CH.SYSTEM_AUDIO,
  CH.INPUT,
  CH.MIC_INPUT,
  CH.SENSOR
])

const isFrameChannel = (ch: number): boolean => FRAME_CHANNELS.has(ch)

const isPingPong = (ch: number, msgId: number): boolean =>
  ch === CH.CONTROL && (msgId === CTRL_MSG.PING_REQUEST || msgId === CTRL_MSG.PING_RESPONSE)

export type SessionTlsDeps = {
  // Outbound wire write. Both SSL_HANDSHAKE-wrapped handshake bytes and the
  // post-handshake `[ch][flags][len:2BE]<ciphertext>` framing go through here.
  writeRaw: (frame: Buffer) => void

  // Inbound delivery: a fully reassembled decrypted AA message.
  onDecryptedMessage: (channelId: number, flags: number, msgId: number, payload: Buffer) => void

  // TLS lifecycle hooks.
  onSecureConnect: () => void
  onError: (err: Error) => void

  // True while the engine is still in the handshake, flips to false after secureConnect.
  // The bridge needs this to decide between SSL_HANDSHAKE-wrap and the encrypted-frame header.
  isHandshakePhase: () => boolean
}

export class SessionTls {
  private readonly _bridge: TlsBridge
  private readonly _tlsSocket: tls.TLSSocket
  private _writeChain: Promise<void> = Promise.resolve()
  private readonly _channelQueue: Array<{ channelId: number; flags: number }> = []
  private readonly _cleartextFragments = new Map<number, { parts: Buffer[]; flags: number }>()
  private _pendingChannelId = 0
  private _pendingFlags = 0

  private readonly _hsOutBuf: Buffer[] = []
  private _hsFlushScheduled = false

  constructor(private readonly deps: SessionTlsDeps) {
    const { tlsSocket, bridge } = createTlsClient(HU_CERT_PEM, HU_KEY_PEM, (tlsBytes) =>
      this._onOutgoingTlsBytes(tlsBytes)
    )
    this._bridge = bridge
    this._tlsSocket = tlsSocket
    this._wire()
  }

  // Push raw encrypted bytes from the wire into the TLS engine
  injectEncrypted(channelId: number, flags: number, rawPayload: Buffer): void {
    this._channelQueue.push({ channelId, flags })
    this._bridge.injectBytes(rawPayload)
  }

  // Plaintext SSL_HANDSHAKE bytes from the phone. No channel context
  injectHandshakeBytes(payload: Buffer): void {
    this._bridge.injectBytes(payload)
  }

  // Wrap and serialise an encrypted frame. Resolves once the TLS engine has flushed it through the outbound callback
  sendEncrypted(channelId: number, flags: number, cleartext: Buffer): void {
    const sock = this._tlsSocket
    this._writeChain = this._writeChain.then(
      () =>
        new Promise<void>((resolve) => {
          this._pendingChannelId = channelId
          this._pendingFlags = flags
          sock.write(cleartext, () => resolve())
        })
    )
    this._writeChain.catch((err) => {
      if (DEBUG) console.warn('[SessionTls] writeChain rejected:', err)
    })
  }

  // Resolves when all pending TLS writes have been flushed (for graceful shutdown / drain)
  drain(): Promise<void> {
    return this._writeChain
  }

  // ── Internals ─────────────────────────────────────────────────────────────

  private _onOutgoingTlsBytes(tlsBytes: Buffer): void {
    if (this.deps.isHandshakePhase()) {
      this._hsOutBuf.push(tlsBytes)
      if (!this._hsFlushScheduled) {
        this._hsFlushScheduled = true
        setImmediate(() => this._flushHandshake())
      }
      return
    }
    // Post-handshake — write [ch][flags][len:2BE]<ciphertext>
    const header = Buffer.allocUnsafe(4)
    header.writeUInt8(this._pendingChannelId, 0)
    header.writeUInt8(this._pendingFlags, 1)
    header.writeUInt16BE(tlsBytes.length, 2)
    if (DEBUG && (TRACE || !isFrameChannel(this._pendingChannelId))) {
      console.log(
        `[SessionTls] sock→ ENC ch=${this._pendingChannelId} flags=0x${this._pendingFlags.toString(16)} ${tlsBytes.length}B`
      )
    }
    this.deps.writeRaw(Buffer.concat([header, tlsBytes]))
  }

  private _flushHandshake(): void {
    this._hsFlushScheduled = false
    if (this._hsOutBuf.length === 0) return
    const all = Buffer.concat(this._hsOutBuf)
    const n = this._hsOutBuf.length
    this._hsOutBuf.length = 0
    if (DEBUG) {
      const note = n > 1 ? ` coalesced from ${n} chunks` : ''
      console.log(
        `[SessionTls] TLS → phone: ${all.length}B (SSL_HANDSHAKE${note}): ${all.toString('hex')}`
      )
    }
    this.deps.writeRaw(encodeFrame(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.SSL_HANDSHAKE, all))
  }

  private _wire(): void {
    this._tlsSocket.on('data', (chunk: Buffer) => this._onDecryptedChunk(chunk))
    this._tlsSocket.on('error', (err) => {
      if (DEBUG) console.error('[SessionTls] TLS error:', err.message)
      this.deps.onError(err)
    })
    this._tlsSocket.on('secureConnect', () => {
      if (DEBUG) console.log('[SessionTls] TLS handshake complete')
      this.deps.onSecureConnect()
    })
  }

  private _onDecryptedChunk(chunk: Buffer): void {
    const ctx = this._channelQueue.shift()
    if (!ctx) {
      if (DEBUG) {
        console.warn(`[SessionTls] TLS data (${chunk.length}B) without channel ctx — dropping`)
      }
      return
    }
    const isFirst = (ctx.flags & 0x01) !== 0
    const isLast = (ctx.flags & 0x02) !== 0

    // BULK — single-frame message, emit immediately
    if (isFirst && isLast) {
      if (chunk.length < 2) {
        if (DEBUG) console.warn(`[SessionTls] TLS decrypted payload too short (${chunk.length}B)`)
        return
      }
      const msgId = chunk.readUInt16BE(0)
      const payload = chunk.subarray(2)
      if (
        DEBUG &&
        (TRACE || (!isFrameChannel(ctx.channelId) && !isPingPong(ctx.channelId, msgId)))
      ) {
        console.log(
          `[SessionTls] ← ch=${ctx.channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length}`
        )
      }
      this.deps.onDecryptedMessage(ctx.channelId, ctx.flags, msgId, payload)
      return
    }

    // FIRST — start cleartext accumulator for this channel
    if (isFirst && !isLast) {
      this._cleartextFragments.set(ctx.channelId, { parts: [chunk], flags: ctx.flags })
      if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
        console.log(`[SessionTls] frag-start ch=${ctx.channelId} have=${chunk.length}B`)
      }
      return
    }

    // MIDDLE / LAST — append cleartext
    const state = this._cleartextFragments.get(ctx.channelId)
    if (!state) {
      if (DEBUG) {
        console.warn(
          `[SessionTls] ch=${ctx.channelId} cleartext continuation without first fragment — dropping`
        )
      }
      return
    }
    state.parts.push(chunk)

    if (!isLast) {
      if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
        const have = state.parts.reduce((n, p) => n + p.length, 0)
        console.log(`[SessionTls] frag-cont  ch=${ctx.channelId} have=${have}B`)
      }
      return
    }

    // LAST — concat all cleartext fragments and emit
    this._cleartextFragments.delete(ctx.channelId)
    const full = Buffer.concat(state.parts)
    if (full.length < 2) {
      if (DEBUG) {
        console.warn(
          `[SessionTls] ch=${ctx.channelId} reassembled cleartext too short (${full.length}B)`
        )
      }
      return
    }
    const msgId = full.readUInt16BE(0)
    const payload = full.subarray(2)
    if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
      console.log(
        `[SessionTls] ← ch=${ctx.channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length} (reassembled from ${state.parts.length} fragments)`
      )
    }
    // Use FIRST fragment's flags — only first/last bits differ across fragments
    this.deps.onDecryptedMessage(ctx.channelId, state.flags, msgId, payload)
  }
}
