/**
 * Telemetry transport over Socket.IO — client side.
 *
 * The counterpart to `TelemetrySocket` (Socket.ts), which hosts the socket. This class
 * instead dials out to a remote LIVI (or anything speaking the same protocol) and merges
 * whatever it broadcasts into the local store, so the rest of the app (adapters,
 * Dashboard) doesn't need to know or care whether telemetry is locally hosted or
 * consumed from a remote host.
 *
 * Inbound:
 *   socket.on('telemetry:update', payload) → store.merge(payload)
 *
 * This is deliberately receive-only: it does not push this instance's own local
 * telemetry (`telemetry:push`) up to the remote host.
 */

import type { TelemetryPayload } from '@shared/types/Telemetry'
import { type Socket as ClientSocket, io } from 'socket.io-client'
import { TelemetryEvents } from './Socket'
import type { TelemetryStore } from './telemetry/TelemetryStore'

export class TelemetrySocketClient {
  socket: ClientSocket | null = null

  constructor(
    private readonly store: TelemetryStore,
    private readonly host: string,
    private readonly port: number
  ) {
    this.startClient()
  }

  private startClient(): void {
    this.socket = io(`http://${this.host}:${this.port}`, {
      reconnection: true,
      reconnectionDelay: 1000,
      reconnectionDelayMax: 10000,
      transports: ['websocket', 'polling']
    })

    this.socket.on('connect', () => {
      console.log(`[TelemetrySocketClient] connected to ${this.host}:${this.port}`)
    })

    this.socket.on(TelemetryEvents.Update, (payload: TelemetryPayload) => {
      this.store.merge(payload)
    })

    this.socket.on('disconnect', (reason: string) => {
      console.warn(`[TelemetrySocketClient] disconnected from ${this.host}:${this.port}:`, reason)
    })

    this.socket.on('connect_error', (err: Error) => {
      console.warn(
        `[TelemetrySocketClient] connect error to ${this.host}:${this.port}:`,
        err.message
      )
    })
  }

  async disconnect(): Promise<void> {
    this.socket?.removeAllListeners()
    this.socket?.disconnect()
    this.socket = null
  }

  async connect(): Promise<void> {
    if (this.socket) return
    this.startClient()
  }
}
