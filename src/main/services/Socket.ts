/**
 * Telemetry transport over Socket.IO.
 *
 * Inbound:
 *   socket.on('telemetry:push', payload) → store.merge(payload)
 *
 * Outbound (re-broadcast on every store change):
 *   io.emit('telemetry:update', snapshot)
 *
 */

import type { TelemetryPayload } from '@shared/types/Telemetry'
import http from 'http'
import { Server } from 'socket.io'
import type { TelemetryStore } from './telemetry/TelemetryStore'

export enum TelemetryEvents {
  Connection = 'connection',
  Push = 'telemetry:push',
  Update = 'telemetry:update'
}

export class TelemetrySocket {
  io: Server | null = null
  httpServer: http.Server | null = null

  private unsubscribeStore: (() => void) | null = null

  constructor(
    private readonly store: TelemetryStore,
    private port = 4000
  ) {
    this.startServer()
  }

  private setupListeners(): void {
    this.io?.on(TelemetryEvents.Connection, (socket) => {
      const snapshot = this.store.snapshot()
      if (Object.keys(snapshot).length > 0) {
        socket.emit(TelemetryEvents.Update, snapshot)
      }
      socket.on(TelemetryEvents.Push, (payload: TelemetryPayload) => {
        this.store.merge(payload)
      })
    })

    // Re-broadcast every merged snapshot to all socket.io clients.
    const onChange = (_patch: TelemetryPayload, snapshot: TelemetryPayload): void => {
      this.io?.emit(TelemetryEvents.Update, snapshot)
    }
    this.store.on('change', onChange)
    this.unsubscribeStore = (): void => {
      this.store.off('change', onChange)
    }
  }

  private startServer(): void {
    this.httpServer = http.createServer()
    this.io = new Server(this.httpServer, { cors: { origin: '*' } })
    this.setupListeners()
    this.httpServer.listen(this.port, () => {
      console.log(`[TelemetrySocket] Server listening on port ${this.port}`)
    })
  }

  async disconnect(): Promise<void> {
    return new Promise((resolve) => {
      this.unsubscribeStore?.()
      this.unsubscribeStore = null
      if (this.io) this.io.close(() => console.log('[TelemetrySocket] IO closed'))
      if (this.httpServer) {
        this.httpServer.close(() => {
          console.log('[TelemetrySocket] HTTP server closed')
          this.io = null
          this.httpServer = null
          resolve()
        })
      } else {
        resolve()
      }
    })
  }

  async connect(): Promise<void> {
    await new Promise((r) => setTimeout(r, 200))
    this.startServer()
  }
}
