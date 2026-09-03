import type { AudioCommand } from './ProjectionEnums'

export type CoreAudioData = {
  command?: AudioCommand
  decodeType: number
  audioType: number
}
