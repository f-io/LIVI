/**
 * TCP server that accepts Android Auto connections on port 5277.
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { TCP_PORT } from '../constants.js'
import { Session, type SessionConfig } from '../session/Session.js'

export class TcpServer extends EventEmitter {
  private _server: net.Server | null = null

  constructor(private readonly _cfg: SessionConfig) {
    super()
  }

  listen(port = TCP_PORT): void {
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
