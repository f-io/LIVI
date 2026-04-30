/**
 * AA stack — Wireless Android Auto protocol engine for LIVI.
 *
 * Public API:
 *
 *   const aa = new AAStack({ huName: 'LIVI' })
 *
 *   aa.on('session',      (session) => { ... })   // new phone connected
 *   aa.on('video-frame',  (buf, ts) => { ... })   // H.264 NAL unit from first session
 *   aa.on('audio-frame',  (buf, ts, ch, chId) => { ... })   // PCM samples
 *   aa.on('error',        (err) => { ... })
 *
 *   aa.start()                          // begins listening on TCP port 5277
 *   aa.stop()                           // closes the server
 *   aa.sendTouch(action, pointers)      // forward touch event to phone
 *   aa.sendButton(keyCode, down)        // forward HW button event to phone
 *
 * Each Session emits the same events directly if you need per-session control.
 */

import { EventEmitter } from 'node:events'
import type { AudioChannelType } from './channels/AudioChannel'
import type { TouchPointer } from './channels/InputChannel'
import type { MediaPlaybackMetadata, MediaPlaybackStatus } from './channels/MediaInfoChannel'
import { Session, type SessionConfig } from './session/Session'
import { detectBtMac, detectWifiBssid } from './system/hwaddr'
import { TcpServer } from './transport/TcpServer'

export type { AudioChannelType } from './channels/AudioChannel.js'
export { BUTTON_KEY, TOUCH_ACTION, type TouchPointer } from './channels/InputChannel.js'
export type {
  MediaPlaybackMetadata,
  MediaPlaybackState,
  MediaPlaybackStatus
} from './channels/MediaInfoChannel.js'
export { TCP_PORT } from './constants'
export type { SessionConfig } from './session/Session'
export { Session } from './session/Session.js'
export { detectBtMac, detectWifiBssid } from './system/hwaddr'
export { TcpServer } from './transport/TcpServer'

export interface AAStackConfig extends SessionConfig {
  /** TCP port to listen on (default: 5277) */
  port?: number
}

export class AAStack extends EventEmitter {
  // Events:
  //   'session'     (session: Session)            — new phone connected
  //   'video-frame' (buf: Buffer, ts: bigint)     — H.264 NAL from latest session
  //   'audio-frame' (buf: Buffer, ts: bigint,
  //                  channel: AudioChannelType,
  //                  channelId: number)           — PCM from latest session
  //   'connected'   ()                            — latest session is fully up
  //   'disconnected' (reason?: string)            — latest session disconnected
  //   'error'       (err: Error)

  private readonly _server: TcpServer
  private _activeSession: Session | null = null

  constructor(private readonly _cfg: AAStackConfig) {
    super()
    // Auto-fill BT MAC and WiFi BSSID from sysfs if not provided by caller.
    // This means `new AAStack({ huName: 'LIVI' })` just works on any Linux system.
    const resolvedCfg: AAStackConfig = {
      ..._cfg,
      btMacAddress: _cfg.btMacAddress ?? detectBtMac(),
      wifiBssid: _cfg.wifiBssid ?? detectWifiBssid()
    }
    this._server = new TcpServer(resolvedCfg)

    this._server.on('session', (session: Session) => {
      this._activeSession = session

      session.on('video-frame', (buf: Buffer, ts: bigint) => this.emit('video-frame', buf, ts))
      session.on(
        'audio-frame',
        (buf: Buffer, ts: bigint, channel: AudioChannelType, channelId: number) =>
          this.emit('audio-frame', buf, ts, channel, channelId)
      )
      session.on('audio-start', (channel: AudioChannelType, channelId: number) =>
        this.emit('audio-start', channel, channelId)
      )
      session.on('audio-stop', (channel: AudioChannelType, channelId: number) =>
        this.emit('audio-stop', channel, channelId)
      )
      session.on('mic-start', (channelId: number) => this.emit('mic-start', channelId))
      session.on('mic-stop', (channelId: number) => this.emit('mic-stop', channelId))
      session.on('host-ui-requested', () => this.emit('host-ui-requested'))
      session.on('media-metadata', (m: MediaPlaybackMetadata) => this.emit('media-metadata', m))
      session.on('media-status', (s: MediaPlaybackStatus) => this.emit('media-status', s))
      session.on('connected', () => this.emit('connected'))
      session.on('disconnected', (reason?: string) => this.emit('disconnected', reason))
      session.on('error', (err: Error) => this.emit('error', err))

      this.emit('session', session)
    })

    this._server.on('error', (err: Error) => this.emit('error', err))
  }

  /** Start the TCP listener. Call once. */
  start(): void {
    this._server.listen(this._cfg.port)
  }

  /** Stop the TCP listener. */
  stop(): void {
    this._server.close()
  }

  /** The most recently connected Session, or null if none. */
  get activeSession(): Session | null {
    return this._activeSession
  }

  /**
   * Forward a touch event to the active session's phone.
   * No-op if no session is RUNNING.
   * @param action   one of TOUCH_ACTION.{DOWN,MOVE,UP}
   * @param pointers absolute pixel coordinates in advertised touchscreen space
   */
  sendTouch(action: number, pointers: TouchPointer[]): void {
    this._activeSession?.sendTouch(action, pointers)
  }

  /**
   * Forward a HW button (key) event to the active session's phone.
   * @param keyCode one of BUTTON_KEY.*
   * @param down    true = press, false = release
   */
  sendButton(keyCode: number | readonly number[], down: boolean): void {
    this._activeSession?.sendButton(keyCode, down)
  }

  /**
   * Forward captured mic PCM (s16le, 16 kHz mono) to the phone.
   * Only effective between 'mic-start' and 'mic-stop' events; outside that
   * window the MicChannel drops frames silently.
   */
  sendMicPcm(buf: Buffer, ts?: bigint): void {
    this._activeSession?.sendMicPcm(buf, ts)
  }

  /**
   * Ask the phone for a fresh H.264 keyframe — call this when the renderer
   * (re)mounts so it doesn't have to wait for the next scheduled IDR.
   * No-op if no session is RUNNING.
   */
  requestKeyframe(): void {
    this._activeSession?.requestKeyframe()
  }

  /**
   * Politely close the AA session: sends ByeByeRequest(USER_SELECTION) on the
   * control channel and walks the state machine to CLOSED. Phone tears its
   * side down and Wi-Fi/BT supervisors stay up so the user can re-launch.
   * No-op if no session is currently active.
   */
  requestShutdown(): void {
    this._activeSession?.requestShutdown()
  }
}
