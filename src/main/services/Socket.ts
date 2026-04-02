import type { TelemetryPayload } from '@shared/types'
import { EventEmitter } from 'events'
import http from 'http'
import { Server } from 'socket.io'

export enum TelemetryEvents {
  Connection = 'connection',

  // external -> main
  Push = 'telemetry:push',

  // main -> clients
  Update = 'telemetry:update',
  Reverse = 'telemetry:reverse',
  Lights = 'telemetry:lights'
}

export class TelemetrySocket extends EventEmitter {
  io: Server | null = null
  httpServer: http.Server | null = null

  private last: TelemetryPayload | null = null
  private lastReverse: boolean | null = null
  private lastLights: boolean | null = null

  constructor(private port = 4000) {
    super()
    this.startServer()
  }

  private setupListeners() {
    this.io?.on(TelemetryEvents.Connection, (socket) => {
      if (this.last) {
        socket.emit(TelemetryEvents.Update, this.last)
      }

      socket.on(TelemetryEvents.Push, (payload: TelemetryPayload) => {
        this.emit(TelemetryEvents.Push, payload)
        this.publishTelemetry(payload)
      })
    })
  }

  private startServer() {
    this.httpServer = http.createServer()
    this.io = new Server(this.httpServer, { cors: { origin: '*' } })
    this.setupListeners()
    this.httpServer.listen(this.port, () => {
      console.log(`[TelemetrySocket] Server listening on port ${this.port}`)
    })
  }

  async disconnect(): Promise<void> {
    return new Promise((resolve) => {
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

  // main -> all clients
  publishTelemetry(payload: TelemetryPayload) {
    const msg: TelemetryPayload = { ts: Date.now(), ...payload }

    this.last = msg
    this.io?.emit(TelemetryEvents.Update, msg)

    const reverse =
      typeof msg.reverse === 'boolean' ? msg.reverse : msg.gear === 'R' || msg.gear === -1

    if (typeof reverse === 'boolean' && reverse !== this.lastReverse) {
      this.lastReverse = reverse
      this.io?.emit(TelemetryEvents.Reverse, reverse)
    }

    if (typeof msg.lights === 'boolean' && msg.lights !== this.lastLights) {
      this.lastLights = msg.lights
      this.io?.emit(TelemetryEvents.Lights, msg.lights)
    }
  }

  publishReverse(reverse: boolean) {
    if (reverse !== this.lastReverse) {
      this.lastReverse = reverse
      if (this.last) this.last = { ...this.last, reverse }
    }
    this.io?.emit(TelemetryEvents.Reverse, reverse)
  }

  publishLights(lights: boolean) {
    if (lights !== this.lastLights) {
      this.lastLights = lights
      if (this.last) this.last = { ...this.last, lights }
    }
    this.io?.emit(TelemetryEvents.Lights, lights)
  }
}
