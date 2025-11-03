import { ExtraConfig } from './Globals'
import { Server } from 'socket.io'
import { EventEmitter } from 'events'
import type { Stream } from 'stream'
import http from 'http'

export enum MessageNames {
  Connection = 'connection',
  GetSettings = 'getSettings',
  SaveSettings = 'saveSettings',
  Stream = 'stream'
}

export class Socket extends EventEmitter {
  config: ExtraConfig
  saveSettings: (settings: ExtraConfig) => void

  io: Server | null = null
  httpServer: http.Server | null = null

  constructor(config: ExtraConfig, saveSettings: (settings: ExtraConfig) => void) {
    super()
    this.config = config
    this.saveSettings = saveSettings
    this.startServer()
  }

  private setupListeners() {
    this.io?.on(MessageNames.Connection, (socket) => {
      this.sendSettings()
      socket.on(MessageNames.GetSettings, () => this.sendSettings())
      socket.on(MessageNames.SaveSettings, (settings: ExtraConfig) => this.saveSettings(settings))
      socket.on(MessageNames.Stream, (stream: Stream) => this.emit(MessageNames.Stream, stream))
    })
  }

  private startServer() {
    this.httpServer = http.createServer()
    this.io = new Server(this.httpServer, { cors: { origin: '*' } })
    this.setupListeners()
    this.httpServer.listen(4000, () => {
      console.log('[Socket] Server listening on port 4000')
    })
  }

  async disconnect(): Promise<void> {
    return new Promise((resolve) => {
      if (this.io) {
        this.io.close(() => {
          console.log('[Socket] IO closed')
        })
      }
      if (this.httpServer) {
        this.httpServer.close(() => {
          console.log('[Socket] HTTP server closed')
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
    return Promise.resolve()
  }

  sendSettings() {
    this.io?.emit('settings', this.config)
  }

  sendReverse(reverse: boolean) {
    this.io?.emit('reverse', reverse)
  }

  sendLights(lights: boolean) {
    this.io?.emit('lights', lights)
  }
}
