import { DEBUG } from '@main/constants'
import { AudioOutput, downsampleToMono, Microphone } from '@main/services/audio'
import type { ExtraConfig } from '@shared/types'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import { AudioData, decodeTypeMap } from '../messages'

export type PlayerKey = string
export type LogicalStreamKey = 'music' | 'nav' | 'voiceAssistant' | 'call'

type VolumeState = Record<LogicalStreamKey, number>

type MusicFadeState = {
  current: number
  target: number
  remainingSamples: number
}

type SendProjectionEvent = (payload: unknown) => void

type SendChunked = (
  channel: string,
  data: ArrayBuffer,
  chunkSize: number,
  extra?: Record<string, unknown>
) => void

type SendMicPcm = (pcm: Int16Array, decodeType: number) => void

export class ProjectionAudio {
  // One AudioOutput per (sampleRate, channels). The OS audio sink (PulseAudio
  // on Linux, CoreAudio on macOS, WASAPI on Windows) mixes all open streams
  // automatically
  private audioPlayers = new Map<PlayerKey, AudioOutput>()
  private lastStreamLogKey: PlayerKey | null = null
  private lastCallPlaybackLog: { decodeType?: number; audioType?: number } | null = null

  // Last used players per logical stream (for clean teardown)
  private lastMusicPlayerKey: PlayerKey | null = null
  private lastNavPlayerKey: PlayerKey | null = null
  private lastVoiceAssistantPlayerKey: PlayerKey | null = null
  private lastCallPlayerKey: PlayerKey | null = null

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 1.0,
    voiceAssistant: 1.0,
    call: 1.0
  }

  // Voice-assistant / phonecall / nav state
  private voiceAssistantActive = false
  private phonecallActive = false
  private navActive = false

  // UI hint state
  private uiCallIncoming = false
  private uiVoiceAssistantHintActive = false
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

  // If we see a long gap between music chunks, we hard-reset the music
  // AudioOutput to flush stale buffer state.
  private readonly musicGapResetMs = process.platform === 'darwin' ? 1000 : 500
  private lastMusicDataAt = 0

  // After nav stop: delay restoring music until this timestamp
  private navHoldUntil = 0

  // When to start the next music ramp
  private nextMusicRampStartAt = 0
  private musicRampActive = false

  // Tracks whether we have been outputting muted (zero) music frames
  private musicGateMuted = false

  // After AudioMediaStart, keep outputting muted frames for a bit so the sink
  // can resync.
  private readonly musicResumeWarmupMs = 1000
  private musicWarmupUntil = 0

  // Wire-tag of music / nav stream (learned from *Start commands).
  private musicAudioType: number | null = null
  private navAudioType: number | null = null

  private musicFade: MusicFadeState = {
    current: 1,
    target: 1,
    remainingSamples: 0
  }

  private audioInfoSent = false
  private _mic: Microphone | null = null
  private currentMicDecodeType: number | null = null

  // Visualizer / FFT toggle
  private visualizerEnabled = false

  constructor(
    private readonly getConfig: () => ExtraConfig,
    private readonly sendProjectionEvent: SendProjectionEvent,
    private readonly sendChunked: SendChunked,
    private readonly sendMicPcm: SendMicPcm
  ) {}

  public setVisualizerEnabled(enabled: boolean) {
    this.visualizerEnabled = !!enabled
  }

  private emitAttention(
    kind: 'call' | 'voiceAssistant' | 'nav',
    active: boolean,
    extra?: Record<string, unknown>
  ) {
    this.sendProjectionEvent({
      type: 'attention',
      payload: {
        kind,
        active,
        ...(extra ?? {})
      }
    })
  }

  // Called from ProjectionService when a new projection session starts
  public resetForSessionStart() {
    this.stopAllAudioPlayers()
    this._mic?.stop()

    this.voiceAssistantActive = false
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
    this.musicAudioType = null
    this.navAudioType = null

    this.lastStreamLogKey = null
    this.lastCallPlaybackLog = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastVoiceAssistantPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false
    this.currentMicDecodeType = null

    this.uiCallIncoming = false
    this.uiVoiceAssistantHintActive = false
    this.uiNavHintActive = false
  }

  // Called from ProjectionService when a projection session stops
  public resetForSessionStop() {
    this.stopAllAudioPlayers()
    this._mic?.stop()

    this.voiceAssistantActive = false
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
    this.musicAudioType = null
    this.navAudioType = null

    this.lastStreamLogKey = null
    this.lastCallPlaybackLog = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastVoiceAssistantPlayerKey = null
    this.lastCallPlayerKey = null

    this.audioInfoSent = false
    this.currentMicDecodeType = null

    this.uiCallIncoming = false
    this.uiVoiceAssistantHintActive = false
    this.uiNavHintActive = false
  }

  public setInitialVolumes(volumes: Partial<VolumeState>) {
    const next: VolumeState = {
      music: typeof volumes.music === 'number' ? volumes.music : this.volumes.music,
      nav: typeof volumes.nav === 'number' ? volumes.nav : this.volumes.nav,
      voiceAssistant:
        typeof volumes.voiceAssistant === 'number'
          ? volumes.voiceAssistant
          : this.volumes.voiceAssistant,
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
  }

  private getRampMsForTransition(from: number, to: number): number {
    return from > to ? this.musicRampDownMs : this.musicRampUpMs
  }

  // Main entrypoint from ProjectionService for audio messages.
  public handleAudioData(msg: AudioData) {
    const meta = msg.decodeType != null ? this.safeDecodeType(msg.decodeType) : null

    // PCM downlink / output (music, nav, voiceAssistant, phone, …)
    if (msg.data) {
      const now = Date.now()
      const logicalKey = this.getLogicalStreamKey(msg)

      // drain buffer
      if (logicalKey === 'music' && !this.mediaActive) {
        return
      }

      // One player per (audioType, rate, channels); OS sink mixes parallel streams.
      const audioTypeKey = msg.audioType ?? 0
      let player = this.getAudioOutputForStream(logicalKey, audioTypeKey, msg)
      if (!player) return

      const volume = this.volumes[logicalKey] ?? 1.0

      if (meta) {
        const keyForStream: PlayerKey = `${logicalKey}:at${audioTypeKey}:${meta.frequency}:${meta.channel}`
        if (logicalKey === 'music') {
          this.lastMusicPlayerKey = keyForStream
        } else if (logicalKey === 'nav') {
          this.lastNavPlayerKey = keyForStream
        } else if (logicalKey === 'voiceAssistant') {
          this.lastVoiceAssistantPlayerKey = keyForStream
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

        this.lastMusicDataAt = now

        const gateUntil = Math.max(this.nextMusicRampStartAt, this.musicWarmupUntil)
        const isGatedMute = gateUntil > 0 && now < gateUntil

        if (isGatedMute) {
          this.musicGateMuted = true
          pcm = new Int16Array(totalSamples)
        } else {
          const fade = this.musicFade

          // First chunk after gate-mute → start the ramp from 0.
          if (this.musicGateMuted) {
            this.musicGateMuted = false
            fade.current = 0
            fade.target = this.navActive ? this.navDuckingTarget : 1
            fade.remainingSamples = 0
            const rampMs = this.getRampMsForTransition(fade.current, fade.target)
            fade.remainingSamples = Math.max(1, Math.round((rampMs / 1000) * sampleRate * channels))
            this.musicRampActive = true
          }

          // Ducking: navActive lowers, navHoldUntil debounces restore.
          const canDuckNow = this.navActive
          const canRestoreNow =
            !this.navActive && (this.navHoldUntil === 0 || now >= this.navHoldUntil)

          let desiredTarget: number
          if (canDuckNow) {
            desiredTarget = this.navDuckingTarget
          } else if (canRestoreNow) {
            desiredTarget = 1
          } else {
            desiredTarget = fade.target
          }

          if (fade.target !== desiredTarget) {
            const rampMs = this.getRampMsForTransition(fade.current, desiredTarget)
            fade.target = desiredTarget
            fade.remainingSamples = Math.max(1, Math.round((rampMs / 1000) * sampleRate * channels))
            this.musicRampActive = true
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
          }

          if (!this.musicRampActive) {
            // Steady state — single multiplication per sample.
            pcm = this.applyGain(msg.data, baseGain * fade.current)
          } else {
            // Ramp in progress — interpolate gain across the chunk.
            pcm = new Int16Array(totalSamples)
            let current = fade.current
            let remaining = fade.remainingSamples
            const target = fade.target
            const needsRamp = remaining > 0 && Math.abs(current - target) > 1e-3
            const step = needsRamp ? (target - current) / remaining : 0

            for (let i = 0; i < totalSamples; i += 1) {
              let v = msg.data[i] * (baseGain * current)
              if (v > 32767) v = 32767
              else if (v < -32768) v = -32768
              pcm[i] = v

              if (needsRamp && remaining > 0) {
                current += step
                remaining -= 1
              } else {
                current = target
              }
            }

            fade.current = current
            fade.remainingSamples = remaining

            if (fade.remainingSamples === 0 || Math.abs(fade.current - fade.target) < 1e-3) {
              fade.current = fade.target
              this.musicRampActive = false
            }
          }
        }
      } else {
        // nav / voiceAssistant / call: single multiplication. The OS sink mixes this
        // stream with whatever music player is also writing.
        pcm = this.applyGain(msg.data, baseGain)
      }

      if (DEBUG && logicalKey === 'call') {
        const nextLogState = {
          decodeType: msg.decodeType,
          audioType: msg.audioType
        }

        const changed =
          !this.lastCallPlaybackLog ||
          this.lastCallPlaybackLog.decodeType !== nextLogState.decodeType ||
          this.lastCallPlaybackLog.audioType !== nextLogState.audioType

        if (changed) {
          console.debug('[ProjectionAudio] call playback write', {
            ...nextLogState,
            samples: pcm.length
          })
          this.lastCallPlaybackLog = nextLogState
        }
      }

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
          this.sendChunked('projection-audio-chunk', mono.buffer as ArrayBuffer, 64 * 1024, {
            sampleRate: inSampleRate,
            channels: 1
          })
        }
      }

      if (!this.audioInfoSent && meta) {
        this.sendProjectionEvent({
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

    // Command-only messages: voice-assistant / phone / media / nav control
    if (msg.command != null) {
      const cmd = msg.command

      if (DEBUG) {
        console.debug('[ProjectionAudio] audio command', {
          ts: Date.now(),
          cmd,
          decodeType: msg.decodeType,
          audioType: msg.audioType,
          voiceAssistantActive: this.voiceAssistantActive,
          phonecallActive: this.phonecallActive
        })
      }

      // Incoming call: pre-accept / ringing
      if (cmd === AudioCommand.AudioAttentionStart || cmd === AudioCommand.AudioAttentionRinging) {
        if (!this.uiCallIncoming) {
          this.uiCallIncoming = true
          this.emitAttention('call', true, { phase: 'incoming' })
        }
      }

      if (cmd === AudioCommand.AudioPhonecallStop) {
        if (this.uiCallIncoming) {
          this.uiCallIncoming = false
          this.emitAttention('call', false, { phase: 'ended' })
        }
      }

      if (cmd === AudioCommand.AudioVoiceAssistantStart) {
        if (!this.uiVoiceAssistantHintActive) {
          this.uiVoiceAssistantHintActive = true
          this.emitAttention('voiceAssistant', true)
        }
      }

      if (cmd === AudioCommand.AudioVoiceAssistantStop) {
        this.voiceAssistantActive = false
      }

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

      if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
        this.navActive = true
        this.navHoldUntil = 0
        if (msg.audioType != null) this.navAudioType = msg.audioType

        if (this.mediaActive && !this.voiceAssistantActive && !this.phonecallActive) {
          this.musicRampActive = true
          this.musicFade.target = this.navDuckingTarget
          this.musicFade.remainingSamples = 0
        }
        return
      }

      // AudioOpen — arm next AudioMediaStart. Don't learn musicAudioType here
      // (fires for every stream open).
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
        if (msg.audioType != null) this.musicAudioType = msg.audioType

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
        // The phone often stops music while voice-assistant/phone is active. Don't keep
        // mediaActive=true — otherwise we ignore the next AudioMediaStart.
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
        this.musicAudioType = null

        if (this.lastMusicPlayerKey) {
          this.stopPlayerByKey(this.lastMusicPlayerKey)
          this.lastMusicPlayerKey = null
        }

        return
      }

      if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
        this.navActive = false
        // Carlinkit fires cmd=7 then cmd=16 ~2s apart — arm timer once.
        if (this.navHoldUntil === 0) {
          this.navHoldUntil = Date.now() + this.navResumeDelayMs
        }
        if (!this.mediaActive && this.lastNavPlayerKey) {
          this.stopPlayerByKey(this.lastNavPlayerKey)
          this.lastNavPlayerKey = null
        }
        return
      }

      if (cmd === AudioCommand.AudioOutputStop) {
        // Only tear down the player for the stream that's closing.
        const stoppingType = msg.audioType ?? null

        const stopMusic =
          stoppingType === null ||
          (this.musicAudioType != null && stoppingType === this.musicAudioType)
        const stopNav =
          stoppingType === null || (this.navAudioType != null && stoppingType === this.navAudioType)

        if (stopMusic && this.lastMusicPlayerKey) {
          this.stopPlayerByKey(this.lastMusicPlayerKey)
          this.lastMusicPlayerKey = null
        }
        if (stopNav && this.lastNavPlayerKey) {
          this.stopPlayerByKey(this.lastNavPlayerKey)
          this.lastNavPlayerKey = null
        }
        if (stoppingType === null && !this.phonecallActive && !this.voiceAssistantActive) {
          if (this.lastVoiceAssistantPlayerKey) {
            this.stopPlayerByKey(this.lastVoiceAssistantPlayerKey)
            this.lastVoiceAssistantPlayerKey = null
          }
          if (this.lastCallPlayerKey) {
            this.stopPlayerByKey(this.lastCallPlayerKey)
            this.lastCallPlayerKey = null
          }
        }
        return
      }

      if (cmd === AudioCommand.AudioInputConfig) {
        if (msg.decodeType != null) {
          const nextMicDecodeType = msg.decodeType
          const decodeTypeChanged = this.currentMicDecodeType !== nextMicDecodeType
          this.currentMicDecodeType = nextMicDecodeType

          if (DEBUG) {
            console.debug('[ProjectionAudio] mic decodeType updated', {
              ts: Date.now(),
              decodeType: this.currentMicDecodeType,
              decodeTypeChanged
            })
          }

          if (decodeTypeChanged && this._mic && this._mic.isCapturing()) {
            this._mic.start(this.currentMicDecodeType)
          }
        }
        return
      }

      if (
        cmd === AudioCommand.AudioVoiceAssistantStart ||
        cmd === AudioCommand.AudioPhonecallStart
      ) {
        const cfg = this.getConfig() as ExtraConfig & {
          micType?: number
          audioTransferMode?: boolean
        }

        if (cmd === AudioCommand.AudioVoiceAssistantStart) {
          this.voiceAssistantActive = true
          this.phonecallActive = false
        } else if (cmd === AudioCommand.AudioPhonecallStart) {
          this.phonecallActive = true
          this.voiceAssistantActive = false
        }

        // While voice is active, keep music muted via gate.
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicWarmupUntil = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.musicGateMuted = true

        if (cfg.audioTransferMode || cfg.micType !== 0) {
          this._mic?.stop()
          return
        }

        if (!this._mic) {
          this._mic = new Microphone()

          this._mic.on('data', (data: Buffer) => {
            if (!data || data.byteLength === 0) return
            if (this.currentMicDecodeType == null) return

            const pcm16 = new Int16Array(data.buffer, data.byteOffset, data.byteLength / 2)

            try {
              this.sendMicPcm(pcm16, this.currentMicDecodeType)
            } catch (e) {
              console.error('[ProjectionAudio] failed to send mic audio', e)
            }
          })
        }

        if (msg.decodeType != null) {
          this.currentMicDecodeType = msg.decodeType
        }

        if (this.currentMicDecodeType == null) {
          if (DEBUG) {
            console.debug('[ProjectionAudio] skip mic start without decodeType', {
              ts: Date.now(),
              cmd
            })
          }
          return
        }

        this._mic.start(this.currentMicDecodeType)
        return
      }

      if (cmd === AudioCommand.AudioVoiceAssistantStop || cmd === AudioCommand.AudioPhonecallStop) {
        if (cmd === AudioCommand.AudioVoiceAssistantStop) {
          this.voiceAssistantActive = false
          if (this.lastVoiceAssistantPlayerKey) {
            this.stopPlayerByKey(this.lastVoiceAssistantPlayerKey)
            this.lastVoiceAssistantPlayerKey = null
          }
        } else if (cmd === AudioCommand.AudioPhonecallStop) {
          this.phonecallActive = false
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
    this.lastCallPlaybackLog = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastVoiceAssistantPlayerKey = null
    this.lastCallPlayerKey = null
  }

  private stopPlayerByKey(key: PlayerKey | null) {
    if (!key) return
    const player = this.audioPlayers.get(key)
    if (!player) return

    try {
      player.stop()
    } catch {
      // ignore
    }
    this.audioPlayers.delete(key)
  }

  private createAndStartAudioPlayer(
    logicalKey: LogicalStreamKey,
    audioType: number,
    sampleRate: number,
    channels: number
  ): AudioOutput {
    const key: PlayerKey = `${logicalKey}:at${audioType}:${sampleRate}:${channels}`

    const player = new AudioOutput({
      sampleRate,
      channels
    })
    player.start()
    this.audioPlayers.set(key, player)

    return player
  }

  private getAudioOutputForStream(
    logicalKey: LogicalStreamKey,
    audioType: number,
    msg: AudioData
  ): AudioOutput | null {
    const meta = msg.decodeType != null ? this.safeDecodeType(msg.decodeType) : null
    if (!meta) {
      if (DEBUG) {
        console.warn('[ProjectionAudio] unknown decodeType in AudioData', {
          decodeType: msg.decodeType,
          audioType: msg.audioType
        })
      }
      return null
    }

    const sampleRate = meta.frequency
    const channels = meta.channel
    const key: PlayerKey = `${logicalKey}:at${audioType}:${sampleRate}:${channels}`

    let player = this.audioPlayers.get(key)
    if (!player) {
      if (DEBUG) {
        console.log(
          `[ProjectionAudio] new player logicalKey=${logicalKey} audioType=${audioType} rate=${sampleRate} channels=${channels} decodeType=${msg.decodeType}`
        )
      }
      player = this.createAndStartAudioPlayer(logicalKey, audioType, sampleRate, channels)
    }

    if (this.lastStreamLogKey !== key) {
      this.lastStreamLogKey = key
    }

    return player
  }

  private getLogicalStreamKey(msg: AudioData): LogicalStreamKey {
    if (this.musicAudioType != null && msg.audioType === this.musicAudioType) return 'music'
    if (this.navAudioType != null && msg.audioType === this.navAudioType) return 'nav'
    if (this.phonecallActive) return 'call'
    if (this.voiceAssistantActive) return 'voiceAssistant'
    if (this.navActive) return 'nav'
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

  private getMediaDelay(): number {
    const cfg = this.getConfig() as ExtraConfig & { mediaDelay?: number }
    const raw = cfg.mediaDelay
    return typeof raw === 'number' && Number.isFinite(raw) && raw >= 0 ? raw : 0
  }
}
