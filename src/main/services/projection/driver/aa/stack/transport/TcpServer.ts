/**
 * TCP server that accepts Android Auto connections on port 5277.
 *
 * After the Bluetooth RFCOMM handshake, the phone connects here.
 * One Session is created per connection.
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { TCP_PORT } from '../constants.js'
import { Session, type SessionConfig } from '../session/Session.js'

export class TcpServer extends EventEmitter {
  // Events:
  //   'session'  (session: Session)  — new connection established
  //   'error'    (err: Error)

  private _server: net.Server | null = null

  constructor(private readonly _cfg: SessionConfig) {
    super()
  }

  listen(port = TCP_PORT): void {
    // allowHalfOpen: true — Android phones send a TCP half-close (FIN) after their
    // last control message (e.g. AudioFocusRequest) to signal "done sending control
    // messages, but still listening for HU responses". With the default allowHalfOpen:false,
    // Node.js would auto-close our write side when we receive the phone's FIN, breaking
    // the session before CHANNEL_OPEN_REQUEST exchange can happen.
    this._server = net.createServer({ allowHalfOpen: true }, (sock) => {
      const remote = `${sock.remoteAddress}:${sock.remotePort}`
      console.log(`[TcpServer] connection from ${remote}`)

      sock.setNoDelay(true)
      sock.setTimeout(30_000)

      const session = new Session(sock, this._cfg)

      session.on('error', (err: Error) => console.error(`[Session ${remote}] error:`, err.message))
      session.on('disconnected', (reason?: string) =>
        console.log(`[Session ${remote}] disconnected: ${reason ?? ''}`)
      )

      this.emit('session', session)

      void session.start().catch((err: Error) => {
        console.error(`[Session ${remote}] start error:`, err.message)
      })
    })

    this._server.on('error', (err) => this.emit('error', err))

    this._server.listen(port, '0.0.0.0', () => {
      console.log(`[TcpServer] listening on port ${port}`)
    })
  }

  close(): void {
    this._server?.close()
  }
}
