/** Uploads files to a CarlinKit dongle over USB. The helper bridges the dongle's bulk pipe to a
 *  unix socket (its `dongle-upload` announce); this speaks the CarlinKit wire over that socket:
 *  an Open handshake, then one SendFile per file. Used to drop the LIVI web tools onto a stock
 *  dongle — the exec/telnet bootstrap runs from the dongle's own web UI. */

import net from 'node:net'
import type { HelperSessionEvent, HelperSessionSource } from '@projection/driver/aa/AaManager'
import { buildServerCgiScript } from './assets/LIVI_cgi.js'
import { buildLiviWeb } from './assets/LIVI_web.js'

const MAGIC = 0x55aa_55aa
const TYPE_OPEN = 0x01
const TYPE_SEND_FILE = 0x99

/** Where the LIVI web tools live on the dongle. */
const CGI_PATH = '/tmp/boa/cgi-bin/server.cgi'
const WEB_PATH = '/tmp/boa/www/index.html'

const CONNECT_TIMEOUT_MS = 3000
const DRAIN_MS = 300

/** A 16-byte CarlinKit header (magic, payload length, type, ~type) and the payload. */
function frame(type: number, payload: Buffer): Buffer {
  const head = Buffer.alloc(16)
  head.writeUInt32LE(MAGIC, 0)
  head.writeUInt32LE(payload.length, 4)
  head.writeUInt32LE(type, 8)
  head.writeUInt32LE(~type >>> 0, 12)
  return Buffer.concat([head, payload])
}

/** Open payload: the projection geometry the dongle expects before it takes messages. */
function openPayload(): Buffer {
  const b = Buffer.alloc(28)
  b.writeUInt32LE(800, 0) // width
  b.writeUInt32LE(480, 4) // height
  b.writeUInt32LE(30, 8) // fps
  b.writeUInt32LE(5, 12) // format
  b.writeUInt32LE(49152, 16) // packet max
  b.writeUInt32LE(2, 20) // ibox version
  b.writeUInt32LE(2, 24) // work mode: CarPlay
  return b
}

/** SendFile payload: `[nameLen][name\0][contentLen][content]`. */
function sendFilePayload(remotePath: string, content: Buffer): Buffer {
  const name = Buffer.from(`${remotePath}\0`, 'ascii')
  const nameLen = Buffer.alloc(4)
  nameLen.writeUInt32LE(name.length)
  const contentLen = Buffer.alloc(4)
  contentLen.writeUInt32LE(content.length)
  return Buffer.concat([nameLen, name, contentLen, content])
}

export type UploadResult = { ok: boolean; error?: string }

/** Tracks whether a stock dongle is on the bus and uploads files to it on demand. */
export class DongleUpload {
  private helper: HelperSessionSource | null = null
  private sub: { close(): void } | null = null
  private socketPath: string | null = null
  private serial = ''

  attachHelper(helper: HelperSessionSource | undefined): void {
    this.detachHelper()
    if (!helper) return
    this.helper = helper
    this.sub = helper.subscribe(
      (ev: HelperSessionEvent) => {
        if (ev.event !== 'dongle-upload' || typeof ev.socket !== 'string') return
        this.socketPath = ev.socket
        this.serial = typeof ev.serial === 'string' ? ev.serial : ''
        console.log(`[DongleUpload] dongle ${this.serial || '(no serial)'} available for upload`)
      },
      () => {
        this.sub = null
      }
    )
  }

  detachHelper(): void {
    try {
      this.sub?.close()
    } catch {
      /* already closed */
    }
    this.sub = null
    this.helper = null
    this.socketPath = null
  }

  get available(): boolean {
    return this.socketPath !== null
  }

  /** Drops the LIVI web tools (server.cgi + index.html) onto the dongle. */
  bootstrap(): Promise<UploadResult> {
    return this.send([
      [CGI_PATH, Buffer.from(buildServerCgiScript(), 'utf8')],
      [WEB_PATH, Buffer.from(buildLiviWeb(), 'utf8')]
    ])
  }

  /** Uploads one file to `remotePath` on the dongle. */
  uploadFile(remotePath: string, content: Buffer): Promise<UploadResult> {
    return this.send([[remotePath, content]])
  }

  private send(files: Array<[string, Buffer]>): Promise<UploadResult> {
    const path = this.socketPath
    if (!path) return Promise.resolve({ ok: false, error: 'no dongle on the bus' })

    return new Promise<UploadResult>((resolve) => {
      let done = false
      const finish = (r: UploadResult): void => {
        if (done) return
        done = true
        clearTimeout(timer)
        sock.destroy()
        resolve(r)
      }
      const sock = net.createConnection(path)
      const timer = setTimeout(
        () => finish({ ok: false, error: 'connect timed out' }),
        CONNECT_TIMEOUT_MS
      )
      sock.on('error', (e: Error) => finish({ ok: false, error: e.message }))
      sock.on('connect', () => {
        clearTimeout(timer)
        sock.write(frame(TYPE_OPEN, openPayload()))
        for (const [remotePath, content] of files) {
          sock.write(frame(TYPE_SEND_FILE, sendFilePayload(remotePath, content)))
        }
        // No per-file ack on the wire; the writes are drained, then the socket closes.
        setTimeout(() => finish({ ok: true }), DRAIN_MS)
      })
    })
  }
}
