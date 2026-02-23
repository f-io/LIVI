import { DongleConfig, TouchAction } from '../../../../main/services/carplay/messages'
import type { AudioData as CoreAudioData } from '../../../../main/services/carplay/messages/readable'

export type AudioData = CoreAudioData

/** Key for audio player routing */
export type AudioPlayerKey = string & { __brand: 'AudioPlayerKey' }

/** Messages sent over the dedicated ports (optional helpers) */
export type VideoPortMessage = { type: 'video'; buffer: ArrayBuffer }
export type AudioPortMessage = { type: 'audio'; buffer: ArrayBuffer; decodeType: number }

/** Payload to initialise the CarPlay worker */
export type InitialisePayload = {
  videoPort?: MessagePort
  audioPort: MessagePort
}

/** Payload for handing an audio player/shared buffer to the worker */
export type AudioPlayerPayload = {
  sab: SharedArrayBuffer
  decodeType: number
  audioType: number
}

/** Start command payload */
export type StartPayload = {
  config: Partial<DongleConfig>
}

/** Command strings accepted by the worker */
export type ValidCommand =
  | 'left'
  | 'right'
  | 'next'
  | 'invalid'
  | 'pause'
  | 'play'
  | 'playPause'
  | 'selectDown'
  | 'back'
  | 'down'
  | 'home'
  | 'prev'
  | 'up'
  | 'selectUp'
  | 'acceptPhone'
  | 'rejectPhone'
  | 'siri'
  | 'frame'
  | 'mic'
  | 'deviceFound'
  | 'startRecordAudio'
  | 'stopRecordAudio'
  | 'requestHostUI'
  | 'wifiPair'

export function isValidCommand(cmd: string): cmd is ValidCommand {
  return [
    'left',
    'right',
    'next',
    'invalid',
    'pause',
    'play',
    'playPause',
    'selectDown',
    'back',
    'down',
    'home',
    'prev',
    'up',
    'selectUp',
    'acceptPhone',
    'rejectPhone',
    'siri',
    'frame',
    'mic',
    'deviceFound',
    'startRecordAudio',
    'stopRecordAudio',
    'requestHostUI',
    'wifiPair'
  ].includes(cmd)
}

/** UI-originated key commands */
export type KeyCommand =
  | 'left'
  | 'right'
  | 'selectDown'
  | 'selectUp'
  | 'back'
  | 'down'
  | 'home'
  | 'play'
  | 'pause'
  | 'playPause'
  | 'next'
  | 'prev'
  | 'acceptPhone'
  | 'rejectPhone'
  | 'siri'

/** Commands the UI can post to the CarPlay worker */
export type Command =
  | { type: 'stop' }
  | { type: 'start'; payload: StartPayload }
  | { type: 'touch'; payload: { x: number; y: number; action: TouchAction } }
  | { type: 'initialise'; payload: InitialisePayload }
  | { type: 'audioPlayer'; payload: AudioPlayerPayload }
  | { type: 'audioBuffer'; payload: AudioPlayerPayload }
  | { type: 'microphoneInput'; payload: Int16Array }
  | { type: 'frame' }
  | { type: 'keyCommand'; command: KeyCommand }

/** Messages the CarPlay worker sends back to the UI thread */
export type WorkerToUI =
  | { type: 'plugged' }
  | { type: 'unplugged' }
  | { type: 'failure' }
  | { type: 'requestBuffer'; message: AudioData }
  | { type: 'audio'; message: AudioData }
  | {
      type: 'audioInfo'
      payload: { codec: string; sampleRate: number; channels: number; bitDepth: number }
    }
  | { type: 'pcmData'; payload: ArrayBuffer }
  | { type: 'command'; message?: { value?: number } }
  | {
      type: 'dongleInfo'
      payload: { serial?: string; manufacturer?: string; product?: string; fwVersion?: string }
    }
  | { type: 'resolution'; payload: { width: number; height: number } }

/** Back-compat alias */
export type CarplayWorkerMessage = WorkerToUI

/** Typed worker instance — do not override onmessage, use addEventListener instead */
export type CarPlayWorker = Worker & {
  postMessage(message: Command, transfer?: Transferable[]): void
}

/** USB/IPC events delivered from the preload to the renderer */
export type UsbEvent =
  | { type: 'attach' | 'plugged' | 'detach' | 'unplugged' }
  | { type: 'media'; payload?: unknown }
  | { type: 'resolution'; payload: { width: number; height: number } }
  | { type: 'audioInfo'; payload?: unknown }
  | { type: 'command'; message?: { value?: number } }
  | { type: 'dongleInfo'; payload?: unknown }
