import { AudioCommand } from '@shared/types/ProjectionEnums'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'

export type LiviStatus = {
  projection: {
    active: 'aa' | 'dongle' | null
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
  usb: { phoneConnected: boolean; dongleConnected: boolean }
}

export const STATUS_VERSION = 1

const INITIAL: LiviStatus = {
  projection: { active: null, streaming: false, phoneType: null },
  audio: {
    media: { playing: false },
    speech: { playing: false },
    system: { playing: false }
  },
  phone: { active: false },
  voiceAssistant: { active: false },
  nav: { announcing: false },
  usb: { phoneConnected: false, dongleConnected: false }
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
  private flushTimer: NodeJS.Timeout | null = null
  private readonly debounceMs: number

  constructor(
    private readonly file: string = path.join(app.getPath('userData'), 'statusData.json'),
    opts: { debounceMs?: number; writeInitial?: boolean } = {}
  ) {
    this.debounceMs = opts.debounceMs ?? 50
    if (opts.writeInitial !== false) this.flushNow()
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

  setUsbState(phoneConnected: boolean, dongleConnected: boolean): void {
    this.patch({ usb: { phoneConnected, dongleConnected } })
  }

  setPhoneCall(active: boolean): void {
    this.patch({ phone: { active } })
  }

  setVoiceAssistant(active: boolean): void {
    this.patch({ voiceAssistant: { active } })
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
    if (this.flushTimer) {
      clearTimeout(this.flushTimer)
      this.flushTimer = null
    }
    this.flushNow()
  }

  private patch(p: DeepPartial<LiviStatus>): void {
    this.state = deepMerge(this.state, p)
    this.scheduleFlush()
  }

  private scheduleFlush(): void {
    if (this.flushTimer) return
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null
      this.flushNow()
    }, this.debounceMs)
  }

  private flushNow(): void {
    const out = {
      version: STATUS_VERSION,
      timestamp: new Date().toISOString(),
      payload: this.state
    }
    const tmp = this.file + '.tmp'
    try {
      fs.writeFileSync(tmp, JSON.stringify(out, null, 2), 'utf8')
      fs.renameSync(tmp, this.file)
    } catch (e) {
      console.warn('[StatusFileWriter] write failed (ignored)', e)
    }
  }
}
