import { AudioCommand } from '@shared/types/ProjectionEnums'
import { DebouncedJsonFile } from './DebouncedJsonFile'

export type LiviStatus = {
  projection: {
    active: 'aa' | 'cp' | null
    streaming: boolean
    phoneType: 'CarPlay' | 'AndroidAuto' | null
  }
  audio: {
    media: { playing: boolean }
    speech: { playing: boolean }
    system: { playing: boolean }
  }
  phone: { active: boolean }
  voiceAssistant: { active: boolean }
  nav: { announcing: boolean }
  ui: { path: string }
}

export const STATUS_VERSION = 1

const INITIAL: LiviStatus = {
  ui: { path: '' },
  projection: { active: null, streaming: false, phoneType: null },
  audio: {
    media: { playing: false },
    speech: { playing: false },
    system: { playing: false }
  },
  phone: { active: false },
  voiceAssistant: { active: false },
  nav: { announcing: false }
}

type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] }

function deepMerge<T extends object>(base: T, patch: DeepPartial<T>): T {
  const out: Record<string, unknown> = { ...(base as Record<string, unknown>) }
  for (const k of Object.keys(patch)) {
    const v = (patch as Record<string, unknown>)[k]
    const b = (base as Record<string, unknown>)[k]
    if (v && typeof v === 'object' && !Array.isArray(v) && b && typeof b === 'object') {
      out[k] = deepMerge(b as object, v as object)
    } else {
      out[k] = v
    }
  }
  return out as T
}

export class StatusFileWriter {
  private state: LiviStatus = INITIAL
  private readonly sink: DebouncedJsonFile

  constructor(file?: string, opts: { debounceMs?: number; writeInitial?: boolean } = {}) {
    this.sink = new DebouncedJsonFile(
      () => ({
        version: STATUS_VERSION,
        timestamp: new Date().toISOString(),
        payload: this.state
      }),
      { file, name: 'statusData.json', tag: 'StatusFileWriter', debounceMs: opts.debounceMs ?? 50 }
    )
    if (opts.writeInitial !== false) this.sink.flushNow()
  }

  setProjection(
    active: LiviStatus['projection']['active'],
    phoneType: LiviStatus['projection']['phoneType']
  ): void {
    this.patch({ projection: { active, phoneType } })
  }

  setStreaming(streaming: boolean): void {
    this.patch({ projection: { streaming } })
  }

  setPhoneCall(active: boolean): void {
    this.patch({ phone: { active } })
  }

  setVoiceAssistant(active: boolean): void {
    this.patch({ voiceAssistant: { active } })
  }

  setPath(path: string): void {
    this.patch({ ui: { path } })
  }

  setNavAnnouncing(on: boolean): void {
    this.patch({ nav: { announcing: on } })
  }

  setAudio(channel: 'media' | 'speech' | 'system', playing: boolean): void {
    this.patch({ audio: { [channel]: { playing } } } as DeepPartial<LiviStatus>)
  }

  applyAudioCommand(cmd: AudioCommand): void {
    switch (cmd) {
      case AudioCommand.AudioMediaStart:
        this.setAudio('media', true)
        return
      case AudioCommand.AudioMediaStop:
        this.setAudio('media', false)
        return
      case AudioCommand.AudioPhonecallStart:
      case AudioCommand.AudioAttentionStart:
      case AudioCommand.AudioAttentionRinging:
        this.setPhoneCall(true)
        return
      case AudioCommand.AudioPhonecallStop:
      case AudioCommand.AudioAttentionStop:
        this.setPhoneCall(false)
        return
      case AudioCommand.AudioVoiceAssistantStart:
        this.setVoiceAssistant(true)
        return
      case AudioCommand.AudioVoiceAssistantStop:
        this.setVoiceAssistant(false)
        return
      case AudioCommand.AudioNaviStart:
      case AudioCommand.AudioTurnByTurnStart:
        this.patch({ nav: { announcing: true }, audio: { speech: { playing: true } } })
        return
      case AudioCommand.AudioNaviStop:
      case AudioCommand.AudioTurnByTurnStop:
        this.patch({ nav: { announcing: false }, audio: { speech: { playing: false } } })
        return
      case AudioCommand.AudioOutputStart:
        this.setAudio('system', true)
        return
      case AudioCommand.AudioOutputStop:
        this.setAudio('system', false)
        return
    }
  }

  getState(): LiviStatus {
    return this.state
  }

  flush(): void {
    this.sink.flushNow()
  }

  private patch(p: DeepPartial<LiviStatus>): void {
    this.state = deepMerge(this.state, p)
    this.sink.schedule()
  }
}
