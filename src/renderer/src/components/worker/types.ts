import { DongleConfig, TouchAction } from '../../../../main/carplay/messages'

export type AudioPlayerKey = string & { __brand: 'AudioPlayerKey' }

export type VideoPortMessage = { type: 'video'; buffer: ArrayBuffer }
export type AudioPortMessage = { type: 'audio'; buffer: ArrayBuffer; decodeType: number }

export type CarplayWorkerMessage =
  | { type: 'resolution'; payload: { width: number; height: number } }
  | {
      type: 'audioInfo'
      payload: { codec: string; sampleRate: number; channels: number; bitDepth: number }
    }
  | { type: 'pcmData'; payload: ArrayBuffer }

export type InitialisePayload = {
  videoPort?: MessagePort
  audioPort: MessagePort
}

export type AudioPlayerPayload = {
  sab: SharedArrayBuffer
  decodeType: number
  audioType: number
}

export type StartPayload = {
  config: Partial<DongleConfig>
}

export type ValidCommand =
  | 'left'
  | 'right'
  | 'next'
  | 'invalid'
  | 'pause'
  | 'play'
  | 'selectDown'
  | 'back'
  | 'down'
  | 'home'
  | 'prev'
  | 'up'
  | 'selectUp'
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
    'selectDown',
    'back',
    'down',
    'home',
    'prev',
    'up',
    'selectUp',
    'frame',
    'mic',
    'deviceFound',
    'startRecordAudio',
    'stopRecordAudio',
    'requestHostUI',
    'wifiPair'
  ].includes(cmd)
}

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
  | 'next'
  | 'prev'

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

export interface CarPlayWorker extends Omit<Worker, 'postMessage' | 'onmessage'> {
  postMessage(message: Command, transfer?: Transferable[]): void
  onmessage: ((this: Worker, ev: CarplayWorkerMessage) => any) | null
}
