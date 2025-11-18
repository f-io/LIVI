import { ExtraConfig } from '@main/Globals'

export type SourcesSettingKey = keyof Pick<
  ExtraConfig,
  'wifiType' | 'micType' | 'camera' //  | 'microphone'
>
