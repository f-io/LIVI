import type { EventEmitter } from 'node:events'
import type { Config } from '@shared/types'
import type { InputCommand } from '@shared/types/InputCommand'
import type { Message } from '../messages/readable.js'
import type { SendableMessage } from '../messages/sendable.js'

/**
 * Common contract for all phone-projection drivers (dongle, aa, cp).
 *
 * Each driver is an EventEmitter that, once `start(cfg)` resolves,
 * pumps decoded readable messages and lifecycle events to ProjectionService.
 * ProjectionService is the single meeting point — it bridges driver output
 * into the renderer pipeline regardless of which driver is active.
 *
 * Required events
 * ---------------
 *   'message'        (msg: Message)        — decoded LIVI-domain message
 *   'config-changed' (patch: Partial<Config>)
 *                                          — phone reported a config delta
 *   'failure'        ()                    — driver layer is dead, restart needed
 *
 * Optional driver-specific events (e.g. 'dongle-info', 'targeted-connect-dispatched')
 * remain on the concrete class and are NOT part of this contract.
 */
export interface IPhoneDriver extends EventEmitter {
  /**
   * Bring the driver up with the given runtime config.
   * Resolves to `true` on success, `false` (or rejects) on failure.
   */
  start(cfg: Config): Promise<boolean | void>

  /** Tear down all driver resources. Idempotent. */
  close(): Promise<void>

  /** Send a message towards the phone. Resolves `true` if dispatched. */
  send(msg: SendableMessage): Promise<boolean>

  /** Forward an abstract input command (from BT AVRCP, CAN bridge, etc.) */
  handleInput(command: InputCommand): void

  /** Ask the phone to emit a fresh video keyframe (IDR) so a rebuilt decoder can start. */
  requestKeyframe?(): void

  /** Request focus for the secondary (cluster) video stream. */
  requestClusterFocus?(): void

  /** Forward host-side audio (mic, TTS) towards the phone. */
  sendPhoneAudio?(pcm: Int16Array, decodeType: number): void

  /** Upload the host app icons (120/180/256) to the phone. */
  uploadHostIcons?(icon120: Buffer, icon180: Buffer, icon256: Buffer): void

  /** Soft-disconnect the current phone while keeping the driver armed. */
  disconnectPhone?(): Promise<boolean>
}

export type DriverMessageEvent = (msg: Message) => void
export type DriverConfigChangedEvent = (patch: Partial<Config>) => void
export type DriverFailureEvent = () => void
