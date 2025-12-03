import { AudioData, AudioCommand, decodeTypeMap } from '../messages'
import { ExtraConfig } from '@main/Globals'
import { Microphone, AudioOutput, downsampleToMono } from '@audio'

export type PlayerKey = string
export type LogicalStreamKey = 'music' | 'nav' | 'siri' | 'call'

type VolumeState = Record<LogicalStreamKey, number>

type MusicFadeState = {
  current: number
  target: number
  remainingSamples: number
}

type SendCarplayEvent = (payload: unknown) => void

type SendChunked = (
  channel: string,
  data: ArrayBuffer,
  chunkSize: number,
  extra?: Record<string, unknown>
) => void

type SendMicPcm = (pcm: Int16Array) => void

export class CarplayAudio {
  // One AudioOutput per (sampleRate, channels)
  private audioPlayers = new Map<PlayerKey, AudioOutput>()
  private lastStreamLogKey: PlayerKey | null = null

  // Last used players per logical stream (for clean teardown)
  private lastMusicPlayerKey: PlayerKey | null = null
  private lastNavPlayerKey: PlayerKey | null = null
  private lastSiriPlayerKey: PlayerKey | null = null
  private lastCallPlayerKey: PlayerKey | null = null

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 1.0,
    siri: 1.0,
    call: 1.0
  }

  // Siri / phonecall / nav state
  private siriActive = false
  private phonecallActive = false
  private navActive = false

  // Media session state (music)
  private mediaActive = false
  private audioOpenArmed = false

  // Ramp configuration
  private readonly musicRampInMs = 1000

  // When to start the next music ramp
  private nextMusicRampStartAt = 0
  private musicRampActive = false

  private musicFade: MusicFadeState = {
    current: 1,
    target: 1,
    remainingSamples: 0
  }

  // Queue for nav PCM that should be mixed into music
  private navMixQueue: Int16Array[] = []
  private navMixOffset = 0

  private audioInfoSent = false
  private _mic: Microphone | null = null

  constructor(
    private readonly getConfig: () => ExtraConfig,
    private readonly sendCarplayEvent: SendCarplayEvent,
    private readonly sendChunked: SendChunked,
    private readonly sendMicPcm: SendMicPcm
  ) {}

  // Called from CarplayService when a new CarPlay session starts
  public resetForSessionStart() {
    this.stopAllAudioPlayers()
    this.clearNavMix()

    this.siriActive = false
    this.phonecallActive = false
    this.navActive = false
    this.mediaActive = false
    this.audioOpenArmed = false
    this.musicRampActive = false
    this.nextMusicRampStartAt = 0
    this.musicFade = { current: 1, target: 1, remainingSamples: 0 }

    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false
  }

  // Called from CarplayService when a CarPlay session stops
  public resetForSessionStop() {
    this.stopAllAudioPlayers()
    this.clearNavMix()

    this.siriActive = false
    this.phonecallActive = false
    this.navActive = false
    this.mediaActive = false
    this.audioOpenArmed = false
    this.musicRampActive = false
    this.nextMusicRampStartAt = 0
    this.musicFade = { current: 1, target: 1, remainingSamples: 0 }

    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false
  }

  // Called from CarplayService.start() after config is loaded.
  public setInitialVolumes(volumes: Partial<VolumeState>) {
    const next: VolumeState = {
      music: typeof volumes.music === 'number' ? volumes.music : this.volumes.music,
      nav: typeof volumes.nav === 'number' ? volumes.nav : this.volumes.nav,
      siri: typeof volumes.siri === 'number' ? volumes.siri : this.volumes.siri,
      call: typeof volumes.call === 'number' ? volumes.call : this.volumes.call
    }

    this.volumes = next
  }

  public setStreamVolume(stream: LogicalStreamKey, volume: number) {
    if (!stream) return
    const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
    const prev = this.volumes[stream]

    if (prev !== undefined && Math.abs(prev - v) < 0.0001) {
      return
    }

    this.volumes[stream] = v
    // Volume changes are rare enough to keep this
    console.debug('[CarplayAudio] setStreamVolume', { stream, volume: v })
  }

  // Main entrypoint from CarplayService for audio messages.
  public handleAudioData(msg: AudioData) {
    const meta = msg.decodeType != null ? this.safeDecodeType(msg.decodeType) : null

    // PCM downlink / output (music, nav, siri, phone, …)
    if (msg.data) {
      const now = Date.now()
      const voiceActive = this.siriActive || this.phonecallActive

      // Drop Siri/phone-coded frames when no voice session is active
      if (msg.decodeType === 5 && !voiceActive) {
        return
      }

      const logicalKey = this.getLogicalStreamKey(msg)

      if (logicalKey === 'music' && !this.mediaActive) {
        return
      }

      const player = this.getAudioOutputForStream(msg)
      if (!player) return

      const volume = this.volumes[logicalKey] ?? 1.0

      // Track last player per logical stream for later teardown
      if (meta) {
        const keyForStream: PlayerKey = `${meta.frequency}:${meta.channel}`
        if (logicalKey === 'music') {
          this.lastMusicPlayerKey = keyForStream
        } else if (logicalKey === 'nav') {
          if (!this.mediaActive) this.lastNavPlayerKey = keyForStream
        } else if (logicalKey === 'siri') {
          this.lastSiriPlayerKey = keyForStream
        } else if (logicalKey === 'call') {
          this.lastCallPlayerKey = keyForStream
        }
      }

      const baseGain = this.gainFromVolume(volume)
      let pcm: Int16Array

      if (logicalKey === 'music') {
        const sampleRate = meta?.frequency ?? 48000
        const channels = meta?.channel ?? 2
        const totalSamples = msg.data.length

        // While Siri/phone is active, music is muted
        if (!this.mediaActive || voiceActive) {
          pcm = new Int16Array(totalSamples)
        } else if (this.nextMusicRampStartAt > 0 && now < this.nextMusicRampStartAt) {
          pcm = new Int16Array(totalSamples)
        } else if (!this.musicRampActive) {
          const navVolume = this.volumes.nav ?? 0.5
          const navGain = this.navActive ? this.gainFromVolume(navVolume) : 0

          if (this.navActive && this.navMixQueue.length > 0 && navGain > 0) {
            pcm = this.mixMusicAndNav(msg.data, baseGain, navGain)
          } else {
            pcm = this.applyGain(msg.data, baseGain)
          }
        } else {
          const fade = this.musicFade

          if (fade.remainingSamples === 0 && fade.current < fade.target) {
            fade.target = 1
            fade.remainingSamples = Math.max(
              1,
              Math.round((this.musicRampInMs / 1000) * sampleRate * channels)
            )
            // Ramp start is rare enough to keep this if you want, but it is not critical.
            console.debug('[CarplayAudio] starting music ramp', {
              samples: fade.remainingSamples,
              sampleRate,
              channels
            })
          }

          const navVolume = this.volumes.nav ?? 0.5
          const navGain = this.navActive ? this.gainFromVolume(navVolume) : 0

          pcm = new Int16Array(totalSamples)

          let current = fade.current
          let remaining = fade.remainingSamples
          const target = fade.target

          for (let i = 0; i < totalSamples; i += 1) {
            if (remaining > 0 && current < target) {
              const step = (target - current) / remaining
              current += step
              remaining -= 1
            } else {
              current = target
            }

            const musicSample = msg.data[i] * (baseGain * current)
            let navSample = 0

            if (this.navActive && this.navMixQueue.length > 0 && navGain > 0) {
              let navChunk = this.navMixQueue[0]
              let navOffset = this.navMixOffset

              if (navChunk) {
                navSample = navChunk[navOffset] * navGain
                navOffset += 1

                if (navOffset >= navChunk.length) {
                  this.navMixQueue.shift()
                  navChunk = this.navMixQueue[0] || null
                  navOffset = 0
                }
              }

              this.navMixOffset = navChunk ? navOffset : 0
            }

            let mixed = musicSample + navSample
            if (mixed > 32767) mixed = 32767
            else if (mixed < -32768) mixed = -32768

            pcm[i] = mixed
          }

          fade.current = current
          fade.remainingSamples = remaining

          if (fade.remainingSamples === 0 || fade.current >= fade.target - 1e-3) {
            this.musicRampActive = false
          }
        }
      } else if (logicalKey === 'nav') {
        // Inline nav while music is active
        if (this.mediaActive) {
          if (this.navActive) {
            // Real inline nav: enqueue for mixing into music
            this.navMixQueue.push(msg.data.slice())
          }
          return
        }

        // Nav-only playback (no music): own nav player
        pcm = this.applyGain(msg.data, baseGain)
      } else {
        // siri / call: no ramp, just volume mapping
        pcm = this.applyGain(msg.data, baseGain)
      }

      // Playback
      player.write(pcm)

      // Mono only for FFT visualization
      if (meta && msg.data) {
        const inSampleRate = meta.frequency ?? 48000
        const inChannels = meta.channel ?? 2

        const mono = downsampleToMono(msg.data, {
          inSampleRate,
          inChannels
        })

        if (mono.length > 0) {
          this.sendChunked('carplay-audio-chunk', mono.buffer as ArrayBuffer, 64 * 1024, {
            sampleRate: inSampleRate,
            channels: 1
          })
        }
      }

      if (!this.audioInfoSent && meta) {
        this.sendCarplayEvent({
          type: 'audioInfo',
          payload: {
            codec: meta.format ?? meta.mimeType,
            sampleRate: meta.frequency,
            channels: meta.channel,
            bitDepth: meta.bitDepth
          }
        })
        this.audioInfoSent = true
      }

      return
    }

    // Command-only messages: Siri / phone / media / nav control
    if (msg.command != null) {
      const cmd = msg.command

      // Explicit Nav / turn-by-turn start
      if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
        this.navActive = true
        this.clearNavMix()
        return
      }

      // 1 == AudioOpen: arm exactly one next AudioMediaStart
      if (cmd === AudioCommand.AudioOutputStart) {
        if (this.mediaActive) {
          return
        }

        this.audioOpenArmed = true
        this.mediaActive = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        return
      }

      if (cmd === AudioCommand.AudioMediaStart) {
        const baseDelay = this.getMediaDelay()
        const totalDelayMs = baseDelay

        if (!this.audioOpenArmed) {
          if (!this.mediaActive) {
            // 10 without 1: treat as implicit open+start
            this.mediaActive = true
            this.musicRampActive = true
            this.musicFade.current = 0
            this.musicFade.target = 1
            this.musicFade.remainingSamples = 0
            this.nextMusicRampStartAt = Date.now() + totalDelayMs
          }
          return
        }

        if (this.mediaActive) {
          return
        }

        this.audioOpenArmed = false
        this.mediaActive = true
        this.musicRampActive = true
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.nextMusicRampStartAt = Date.now() + totalDelayMs

        return
      }

      if (cmd === AudioCommand.AudioMediaStop) {
        this.mediaActive = false
        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0

        if (this.lastMusicPlayerKey) {
          this.stopPlayerByKey(this.lastMusicPlayerKey, 'music')
          this.lastMusicPlayerKey = null
        }

        return
      }

      if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
        this.navActive = false
        this.clearNavMix()

        if (!this.mediaActive && this.lastNavPlayerKey) {
          this.stopPlayerByKey(this.lastNavPlayerKey, 'nav')
          this.lastNavPlayerKey = null
        } else {
          // mixing with music -> do not kill shared player, let tail drain
        }
        return
      }

      if (cmd === AudioCommand.AudioSiriStart || cmd === AudioCommand.AudioPhonecallStart) {
        const cfg = this.getConfig()
        if (cfg.audioTransferMode) return

        if (cmd === AudioCommand.AudioSiriStart) {
          this.siriActive = true
          this.phonecallActive = false
        } else if (cmd === AudioCommand.AudioPhonecallStart) {
          this.phonecallActive = true
          this.siriActive = false
        }

        // While voice is active, keep music muted
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0

        if (!this._mic) {
          this._mic = new Microphone()

          this._mic.on('data', (data: Buffer) => {
            if (!data || data.byteLength === 0) return

            const pcm16 = new Int16Array(data.buffer)

            try {
              this.sendMicPcm(pcm16)
            } catch (e) {
              console.error('[CarplayAudio] failed to send mic audio', e)
            }
          })
        }

        this._mic.start()
        return
      }

      if (cmd === AudioCommand.AudioSiriStop || cmd === AudioCommand.AudioPhonecallStop) {
        if (cmd === AudioCommand.AudioSiriStop) {
          this.siriActive = false
          if (this.lastSiriPlayerKey) {
            this.stopPlayerByKey(this.lastSiriPlayerKey, 'siri')
            this.lastSiriPlayerKey = null
          }
        } else if (cmd === AudioCommand.AudioPhonecallStop) {
          this.phonecallActive = false
          if (this.lastCallPlayerKey) {
            this.stopPlayerByKey(this.lastCallPlayerKey, 'call')
            this.lastCallPlayerKey = null
          }
        }

        // After voice: ramp-in for music, if media is active
        if (this.mediaActive) {
          this.musicRampActive = true
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
          this.nextMusicRampStartAt = Date.now()
        } else {
          this.musicRampActive = false
          this.nextMusicRampStartAt = 0
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
        }

        this._mic?.stop()
        return
      }
    }
  }

  private safeDecodeType(decodeType: number) {
    return decodeTypeMap[decodeType]
  }

  private stopAllAudioPlayers() {
    for (const player of this.audioPlayers.values()) {
      try {
        player.stop()
      } catch {
        // ignore
      }
    }
    this.audioPlayers.clear()
    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null
  }

  private stopPlayerByKey(key: PlayerKey | null, label: string) {
    if (!key) return
    const player = this.audioPlayers.get(key)
    if (!player) return

    try {
      player.stop()
    } catch {
      // ignore
    }
    this.audioPlayers.delete(key)

    // on teardown
    console.debug('[CarplayAudio] stopped AudioOutput', {
      label,
      playerKey: key
    })
  }

  private createAndStartAudioPlayer(sampleRate: number, channels: number): AudioOutput {
    const key: PlayerKey = `${sampleRate}:${channels}`

    const player = new AudioOutput({
      sampleRate,
      channels
    })
    player.start()
    this.audioPlayers.set(key, player)

    console.debug('[CarplayAudio] created AudioOutput', {
      playerKey: key,
      sampleRate,
      channels
    })

    return player
  }

  private getAudioOutputForStream(msg: AudioData): AudioOutput | null {
    const meta = msg.decodeType != null ? this.safeDecodeType(msg.decodeType) : null
    if (!meta) {
      console.warn('[CarplayAudio] unknown decodeType in AudioData', {
        decodeType: msg.decodeType,
        audioType: msg.audioType
      })
      return null
    }

    const sampleRate = meta.frequency
    const channels = meta.channel
    const key: PlayerKey = `${sampleRate}:${channels}`

    let player = this.audioPlayers.get(key)
    if (!player) {
      player = this.createAndStartAudioPlayer(sampleRate, channels)
    }

    if (this.lastStreamLogKey !== key) {
      this.lastStreamLogKey = key
      console.debug('[CarplayAudio] using AudioOutput for stream', {
        key,
        decodeType: msg.decodeType,
        audioType: msg.audioType,
        sampleRate,
        channels
      })
    }

    return player
  }

  private getLogicalStreamKey(msg: AudioData): LogicalStreamKey {
    const audioType = msg.audioType ?? 1

    if (audioType === 2) return 'nav'

    if (audioType === 1) {
      if (msg.decodeType === 4) {
        return 'music'
      }

      if (msg.decodeType === 5) {
        if (this.siriActive) return 'siri'
        if (this.phonecallActive) return 'call'
        return 'music'
      }

      if (this.siriActive) return 'siri'
      if (this.phonecallActive) return 'call'
      return 'music'
    }

    if (audioType === 3) return 'siri'
    if (audioType === 4) return 'call'

    return 'music'
  }

  private gainFromVolume(volume: number): number {
    const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
    if (v <= 0) return 0
    const minDb = -60
    const maxDb = 0
    const db = minDb + (maxDb - minDb) * v
    return Math.pow(10, db / 20)
  }

  private applyGain(pcm: Int16Array, gain: number): Int16Array {
    if (!Number.isFinite(gain) || gain === 1.0) {
      return pcm
    }
    if (gain <= 0) {
      return new Int16Array(pcm.length)
    }

    const out = new Int16Array(pcm.length)
    for (let i = 0; i < pcm.length; i += 1) {
      let v = pcm[i] * gain
      if (v > 32767) v = 32767
      else if (v < -32768) v = -32768
      out[i] = v
    }
    return out
  }

  private mixMusicAndNav(musicPcm: Int16Array, musicGain: number, navGain: number): Int16Array {
    if (navGain <= 0 || this.navMixQueue.length === 0) {
      // Nothing to mix, just apply music gain
      return this.applyGain(musicPcm, musicGain)
    }

    const out = new Int16Array(musicPcm.length)

    let navChunk = this.navMixQueue[0]
    let navOffset = this.navMixOffset

    for (let i = 0; i < musicPcm.length; i += 1) {
      const musicSample = musicPcm[i] * musicGain

      let navSample = 0
      if (navChunk) {
        navSample = navChunk[navOffset] * navGain
        navOffset += 1

        if (navOffset >= navChunk.length) {
          this.navMixQueue.shift()
          navChunk = this.navMixQueue[0] || null
          navOffset = 0
        }
      }

      let mixed = musicSample + navSample
      if (mixed > 32767) mixed = 32767
      else if (mixed < -32768) mixed = -32768

      out[i] = mixed
    }

    this.navMixOffset = navChunk ? navOffset : 0

    return out
  }

  private clearNavMix() {
    this.navMixQueue = []
    this.navMixOffset = 0
  }

  private getMediaDelay(): number {
    const cfg = this.getConfig() as ExtraConfig & { mediaDelay?: number }
    const raw = cfg.mediaDelay
    return typeof raw === 'number' && Number.isFinite(raw) && raw >= 0 ? raw : 0
  }
}
