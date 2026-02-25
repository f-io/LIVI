import { AudioData, AudioCommand, decodeTypeMap } from '../messages'
import { ExtraConfig } from '@main/Globals'
import { Microphone, AudioOutput, downsampleToMono } from '@main/services/audio'

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

  // UI hint state
  private uiCallIncoming = false
  private uiSiriHintActive = false
  private uiNavHintActive = false

  // Media session state (music)
  private mediaActive = false
  private audioOpenArmed = false

  // Ramp configuration (asymmetric)
  private readonly musicRampDownMs = 500
  private readonly musicRampUpMs = 1500

  // Music ducking target while nav is active (20%)
  private readonly navDuckingTarget = 0.2

  // Debounce time after nav stop before ramping music back to 1.0
  private readonly navResumeDelayMs = 1500

  // If we see a long gap between music chunks, we hard-reset the music AudioOutput
  // to avoid PipeWire/pw-play buffer state causing stutter on resume.
  private readonly musicGapResetMs = 500
  private lastMusicDataAt = 0

  // After nav stop: delay restoring music until this timestamp
  private navHoldUntil = 0

  // When to start the next music ramp
  private nextMusicRampStartAt = 0
  private musicRampActive = false

  // Tracks whether we have been outputting muted (zero) music frames and should ramp
  private musicGateMuted = false

  // After AudioMediaStart, keep outputting muted frames for a bit so pw-play can resync
  private readonly musicResumeWarmupMs = 1000
  private musicWarmupUntil = 0

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

  // Visualizer / FFT toggle
  private visualizerEnabled = false

  constructor(
    private readonly getConfig: () => ExtraConfig,
    private readonly sendCarplayEvent: SendCarplayEvent,
    private readonly sendChunked: SendChunked,
    private readonly sendMicPcm: SendMicPcm
  ) {}

  public setVisualizerEnabled(enabled: boolean) {
    this.visualizerEnabled = !!enabled
  }

  private emitAttention(
    kind: 'call' | 'siri' | 'nav',
    active: boolean,
    extra?: Record<string, unknown>
  ) {
    this.sendCarplayEvent({
      type: 'attention',
      payload: {
        kind,
        active,
        ...(extra ?? {})
      }
    })
  }

  // Called from CarplayService when a new CarPlay session starts
  public resetForSessionStart() {
    this.stopAllAudioPlayers()
    this.clearNavMix()

    this.siriActive = false
    this.phonecallActive = false
    this.navActive = false
    this.navHoldUntil = 0
    this.mediaActive = false
    this.audioOpenArmed = false
    this.musicRampActive = false
    this.nextMusicRampStartAt = 0
    this.musicFade = { current: 1, target: 1, remainingSamples: 0 }
    this.lastMusicDataAt = 0
    this.musicGateMuted = false
    this.musicWarmupUntil = 0

    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false

    // UI hint state reset
    this.uiCallIncoming = false
    this.uiSiriHintActive = false
    this.uiNavHintActive = false
  }

  // Called from CarplayService when a CarPlay session stops
  public resetForSessionStop() {
    this.stopAllAudioPlayers()
    this.clearNavMix()

    this.siriActive = false
    this.phonecallActive = false
    this.navActive = false
    this.navHoldUntil = 0
    this.mediaActive = false
    this.audioOpenArmed = false
    this.musicRampActive = false
    this.nextMusicRampStartAt = 0
    this.musicFade = { current: 1, target: 1, remainingSamples: 0 }
    this.lastMusicDataAt = 0
    this.musicGateMuted = false
    this.musicWarmupUntil = 0

    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false

    // UI hint state reset
    this.uiCallIncoming = false
    this.uiSiriHintActive = false
    this.uiNavHintActive = false
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
    console.debug('[CarplayAudio] setStreamVolume', { stream, volume: v })
  }

  private getRampMsForTransition(from: number, to: number): number {
    return from > to ? this.musicRampDownMs : this.musicRampUpMs
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

      // Player selection
      // music/nav use their normal stream player
      // siri/call always use their own stream player (PipeWire mixes)
      let player = this.getAudioOutputForStream(msg)
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

        // If music chunks resume after a longer gap,
        // hard-reset the music AudioOutput to flush any buffered state and resync
        if (this.lastMusicDataAt > 0 && now - this.lastMusicDataAt > this.musicGapResetMs) {
          if (this.lastMusicPlayerKey) {
            this.stopPlayerByKey(this.lastMusicPlayerKey, 'music')
            this.lastMusicPlayerKey = null
          }

          // Log on the next AudioOutput selection
          this.lastStreamLogKey = null

          // Reset ramp state
          this.musicRampActive = false
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
          this.musicGateMuted = true

          // If we are already receiving chunks again after a gap, do a short warmup
          this.musicWarmupUntil = Math.max(this.musicWarmupUntil, now + this.musicResumeWarmupMs)

          // Re-acquire player after stopping it
          player = this.getAudioOutputForStream(msg)
          if (!player) return
        }

        this.lastMusicDataAt = now

        const gateUntil = Math.max(this.nextMusicRampStartAt, this.musicWarmupUntil)

        const isGatedMute = !this.mediaActive || voiceActive || (gateUntil > 0 && now < gateUntil)

        if (isGatedMute) {
          this.musicGateMuted = true
          pcm = new Int16Array(totalSamples)
        } else {
          const fade = this.musicFade

          // If we were previously outputting zeros, start the ramp exactly on the first audio chunk
          if (this.musicGateMuted) {
            this.musicGateMuted = false

            fade.current = 0
            fade.target = this.navActive ? this.navDuckingTarget : 1
            fade.remainingSamples = 0

            const rampMs = this.getRampMsForTransition(fade.current, fade.target)
            fade.remainingSamples = Math.max(1, Math.round((rampMs / 1000) * sampleRate * channels))
            this.musicRampActive = true

            console.debug('[CarplayAudio] starting music ramp', {
              samples: fade.remainingSamples,
              sampleRate,
              channels,
              from: fade.current,
              to: fade.target,
              rampMs
            })
          }

          // Debounce semantics:
          // - navActive controls whether we are allowed to duck (ramp down)
          // - navHoldUntil delays restoring (ramp up) after nav stop
          const canDuckNow = this.navActive
          const canRestoreNow =
            !this.navActive && (this.navHoldUntil === 0 || now >= this.navHoldUntil)

          let desiredTarget: number
          if (canDuckNow) {
            desiredTarget = this.navDuckingTarget
          } else if (canRestoreNow) {
            desiredTarget = 1
          } else {
            // In hold window: keep whatever target we already had
            desiredTarget = fade.target
          }

          if (fade.target !== desiredTarget) {
            const rampMs = this.getRampMsForTransition(fade.current, desiredTarget)
            fade.target = desiredTarget
            fade.remainingSamples = Math.max(1, Math.round((rampMs / 1000) * sampleRate * channels))
            this.musicRampActive = true
            console.debug('[CarplayAudio] starting music ramp', {
              samples: fade.remainingSamples,
              sampleRate,
              channels,
              from: fade.current,
              to: fade.target,
              rampMs
            })
          }

          if (
            (this.musicRampActive &&
              fade.remainingSamples === 0 &&
              Math.abs(fade.current - fade.target) > 1e-3) ||
            (!this.musicRampActive && Math.abs(fade.current - fade.target) > 1e-3)
          ) {
            const rampMs = this.getRampMsForTransition(fade.current, fade.target)
            this.musicRampActive = true
            fade.remainingSamples = Math.max(1, Math.round((rampMs / 1000) * sampleRate * channels))
            console.debug('[CarplayAudio] starting music ramp', {
              samples: fade.remainingSamples,
              sampleRate,
              channels,
              from: fade.current,
              to: fade.target,
              rampMs
            })
          }

          const navVolume = this.volumes.nav ?? 0.5
          const navGain = this.navActive ? this.gainFromVolume(navVolume) : 0
          const mixNav = this.navActive && this.navMixQueue.length > 0 && navGain > 0

          if (!this.musicRampActive) {
            const musicGain = baseGain * fade.current
            pcm = this.processMusicChunk(msg.data, musicGain, navGain, mixNav)
          } else {
            pcm = new Int16Array(totalSamples)

            let current = fade.current
            let remaining = fade.remainingSamples
            const target = fade.target
            const needsRamp = remaining > 0 && Math.abs(current - target) > 1e-3
            const step = needsRamp ? (target - current) / remaining : 0

            let navChunk = this.navMixQueue[0]
            let navOffset = this.navMixOffset

            for (let i = 0; i < totalSamples; i += 1) {
              const musicSample = msg.data[i] * (baseGain * current)
              let navSample = 0

              if (mixNav && navChunk) {
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

              pcm[i] = mixed

              if (needsRamp && remaining > 0) {
                current += step
                remaining -= 1
              } else {
                current = target
              }
            }

            fade.current = current
            fade.remainingSamples = remaining
            this.navMixOffset = navChunk ? navOffset : 0

            if (fade.remainingSamples === 0 || Math.abs(fade.current - fade.target) < 1e-3) {
              fade.current = fade.target
              this.musicRampActive = false
            }
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

      // Mono only for FFT visualization (optional)
      if (this.visualizerEnabled && meta && msg.data) {
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

      // UI attention hints (renderer decides what to do)

      // Incoming call: pre-accept / ringing
      if (cmd === AudioCommand.AudioAttentionStart || cmd === AudioCommand.AudioAttentionRinging) {
        if (!this.uiCallIncoming) {
          this.uiCallIncoming = true
          this.emitAttention('call', true, { phase: 'incoming' })
        }
      }

      // Call ended / rejected / finished
      if (cmd === AudioCommand.AudioPhonecallStop) {
        if (this.uiCallIncoming) {
          this.uiCallIncoming = false
          this.emitAttention('call', false, { phase: 'ended' })
        }
      }

      // Siri visual feedback
      if (cmd === AudioCommand.AudioSiriStart) {
        if (!this.uiSiriHintActive) {
          this.uiSiriHintActive = true
          this.emitAttention('siri', true)
        }
      }

      if (cmd === AudioCommand.AudioSiriStop) {
        if (this.uiSiriHintActive) {
          this.uiSiriHintActive = false
          this.emitAttention('siri', false)
        }
      }

      // Navigation overlay hints
      if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
        if (!this.uiNavHintActive) {
          this.uiNavHintActive = true
          this.emitAttention('nav', true)
        }
      }

      if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
        if (this.uiNavHintActive) {
          this.uiNavHintActive = false
          this.emitAttention('nav', false)
        }
      }

      // Explicit Nav / turn-by-turn start
      if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
        this.navActive = true
        this.navHoldUntil = 0
        this.clearNavMix()

        if (this.mediaActive && !this.siriActive && !this.phonecallActive) {
          // We don't compute remainingSamples here; it will be computed on next music chunk
          this.musicRampActive = true
          this.musicFade.target = this.navDuckingTarget
          this.musicFade.remainingSamples = 0
        }

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
        this.lastMusicDataAt = 0
        this.musicGateMuted = true
        this.musicWarmupUntil = 0
        return
      }

      if (cmd === AudioCommand.AudioMediaStart) {
        const baseDelay = this.getMediaDelay()
        const totalDelayMs = baseDelay

        const now = Date.now()
        const warmupUntil = now + totalDelayMs + this.musicResumeWarmupMs

        if (!this.audioOpenArmed) {
          if (!this.mediaActive) {
            // 10 without 1: treat as implicit open+start
            this.mediaActive = true
            this.musicRampActive = false
            this.musicFade.current = 0
            this.musicFade.target = 1
            this.musicFade.remainingSamples = 0
            this.nextMusicRampStartAt = now + totalDelayMs
            this.musicWarmupUntil = warmupUntil
            this.musicGateMuted = true
          }
          return
        }

        if (this.mediaActive) {
          return
        }

        this.audioOpenArmed = false
        this.mediaActive = true
        this.musicRampActive = false
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.nextMusicRampStartAt = now + totalDelayMs
        this.musicWarmupUntil = warmupUntil
        this.musicGateMuted = true

        return
      }

      if (cmd === AudioCommand.AudioMediaStop) {
        // IMPORTANT: the phone often stops music while Siri/phone is active.
        // If we keep mediaActive=true, we may ignore the subsequent AudioMediaStart,
        // forcing the user to press stop/play manually.
        this.mediaActive = false

        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicWarmupUntil = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.lastMusicDataAt = 0
        this.musicGateMuted = false

        if (this.lastMusicPlayerKey) {
          this.stopPlayerByKey(this.lastMusicPlayerKey, 'music')
          this.lastMusicPlayerKey = null
        }

        return
      }

      if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
        this.navActive = false
        this.clearNavMix()

        // Debounce: delay restoring music to 1.0, but do NOT make nav "effectively active"
        this.navHoldUntil = Date.now() + this.navResumeDelayMs

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
        this.musicWarmupUntil = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.musicGateMuted = true

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

  private processMusicChunk(
    musicPcm: Int16Array,
    musicGain: number,
    navGain: number,
    mixNav: boolean
  ): Int16Array {
    if (!mixNav) {
      return this.applyGain(musicPcm, musicGain)
    }

    const out = new Int16Array(musicPcm.length)

    let navChunk = this.navMixQueue[0]
    let navOffset = this.navMixOffset

    for (let i = 0; i < musicPcm.length; i += 1) {
      let mixed = musicPcm[i] * musicGain

      if (navChunk) {
        mixed += navChunk[navOffset] * navGain
        navOffset += 1

        if (navOffset >= navChunk.length) {
          this.navMixQueue.shift()
          navChunk = this.navMixQueue[0] || null
          navOffset = 0
        }
      }

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
