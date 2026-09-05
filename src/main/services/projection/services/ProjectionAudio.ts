import { DEBUG } from '@main/constants'
import { HostAudioOutput } from '@main/services/audio'
import { gstHost } from '@main/services/video/gstHost'
import type { Config } from '@shared/types'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import {
  onAudioReceiverVisualizer,
  setAudioReceiverVisualizerTap,
  useHostProcess
} from '../../video/GstVideo'
import type { AudioData } from '../messages'
import type { ProjectionEvent } from './types'

export type PlayerKey = string
export type LogicalStreamKey = 'music' | 'nav' | 'voiceAssistant' | 'call'
type VolumeState = Record<LogicalStreamKey, number>
type SendProjectionEvent = (payload: ProjectionEvent) => void
type SendChunked = (
  channel: string,
  data: ArrayBuffer,
  chunkSize: number,
  extra?: Record<string, unknown>
) => void
type HostOutputListener = (audioType: number, streamId: number, tag?: string) => void
type PrimedOutput = { audioType: number; sampleRate: number; channels: number; tag?: string }
type HostOutput = { audioType: number; streamId: number; tag?: string }

/**
 * The host streams the drivers feed, their levels, and the attention hints the
 * renderer shows. No samples pass through here.
 */
export class ProjectionAudio {
  private readonly players = new Map<PlayerKey, HostAudioOutput>()
  private readonly outputAudioTypes = new WeakMap<HostAudioOutput, number>()
  /** The driver's channel a stream was opened for. */
  private readonly outputTags = new WeakMap<HostAudioOutput, string>()
  private readonly hostOutputListeners = new Set<HostOutputListener>()
  /** What the drivers primed, so a device change reopens the same streams. */
  private readonly primed = new Map<PlayerKey, PrimedOutput>()

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 1.0,
    voiceAssistant: 1.0,
    call: 1.0
  }
  private readonly rampDownMs = 500
  private readonly rampUpMs = 1500
  private duckLevel = 1
  private duckRampMs = this.rampUpMs

  // UI hint state
  private uiCallIncoming = false
  private uiVoiceAssistantHintActive = false
  private uiNavHintActive = false

  private visualizerWindows = new Set<number>()

  constructor(
    private readonly getConfig: () => Config,
    private readonly sendProjectionEvent: SendProjectionEvent,
    private readonly sendChunked: SendChunked,
    private readonly applyStreamVolume: (
      audioType: number,
      level: number,
      rampMs: number
    ) => void = () => {}
  ) {
    // Forward the pre-fader mono tap to the renderer, wherever the audio was received.
    gstHost.onVisualizerAudio((samples, rate) => this.onVisualizerSamples(samples, rate))
    onAudioReceiverVisualizer((samples, rate) => this.onVisualizerSamples(samples, rate))
  }

  private onVisualizerSamples(samples: Uint8Array, sampleRate: number): void {
    if (this.visualizerWindows.size === 0 || samples.length === 0) return
    this.sendChunked('projection-audio-chunk', samples.buffer as ArrayBuffer, 64 * 1024, {
      sampleRate,
      channels: 1
    })
  }

  /** The CarPlay audioType each logical stream travels on. */
  private static readonly AUDIO_TYPE: Record<LogicalStreamKey, number> = {
    music: 3,
    nav: 4,
    voiceAssistant: 1,
    call: 2
  }

  /** Pushes a logical stream's level to the driver that plays it out. */
  private pushStreamVolume(stream: LogicalStreamKey, rampMs = 0): void {
    const duck = stream === 'music' ? this.duckLevel : 1
    this.applyStreamVolume(ProjectionAudio.AUDIO_TYPE[stream], this.volumes[stream] * duck, rampMs)
  }

  /** Pushes every level, after a volume or duck change. */
  private pushAllStreamVolumes(): void {
    for (const stream of Object.keys(ProjectionAudio.AUDIO_TYPE) as LogicalStreamKey[]) {
      this.pushStreamVolume(stream)
    }
  }

  public setVisualizerEnabled(enabled: boolean, sourceId = -1) {
    const had = this.visualizerWindows.size > 0
    if (enabled) this.visualizerWindows.add(sourceId)
    else this.visualizerWindows.delete(sourceId)
    const wants = this.visualizerWindows.size > 0
    if (wants !== had) {
      if (useHostProcess) gstHost.setVisualizerTap(wants)
      else setAudioReceiverVisualizerTap(wants)
    }
  }

  // True while any window wants the FFT chunks
  public get visualizerEnabled(): boolean {
    return this.visualizerWindows.size > 0
  }

  private emitAttention(
    kind: 'call' | 'voiceAssistant' | 'nav',
    active: boolean,
    extra?: { phase?: 'incoming' | 'ended' }
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
    this.resetAudioState()
  }

  // Called from ProjectionService when a projection session stops
  public resetForSessionStop() {
    this.resetAudioState()
  }

  private resetAudioState() {
    this.stopAllPlayers()
    this.duckLevel = 1
    this.duckRampMs = this.rampUpMs
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
    this.pushAllStreamVolumes()
  }

  public setStreamVolume(stream: LogicalStreamKey, volume: number) {
    if (!stream) return
    const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
    const prev = this.volumes[stream]
    if (Math.abs(prev - v) < 0.0001) {
      return
    }
    this.volumes[stream] = v
    this.pushStreamVolume(stream, this.getRampMsForTransition(prev, v))
  }

  private getRampMsForTransition(from: number, to: number): number {
    return from > to ? this.rampDownMs : this.rampUpMs
  }

  public duck(level: number, durationMs: number): void {
    this.duckLevel = Math.max(0, Math.min(1, level))
    this.duckRampMs = Math.max(0, durationMs)
    this.pushStreamVolume('music', this.duckRampMs)
  }

  public unduck(durationMs: number): void {
    this.duckLevel = 1
    this.duckRampMs = Math.max(0, durationMs)
    this.pushStreamVolume('music', this.duckRampMs)
  }

  /** The duck level of the session that just became active. */
  public restoreDuck(level: number, durationMs: number): void {
    this.duckLevel = Math.max(0, Math.min(1, level))
    this.duckRampMs = Math.max(0, durationMs)
    this.pushStreamVolume('music', this.duckRampMs)
  }

  /** Audio commands drive the attention hints, each driver keeps its own stream state. */
  public handleAudioData(msg: AudioData) {
    const cmd = msg.command
    if (cmd == null) return
    if (DEBUG) {
      console.debug('[ProjectionAudio] audio command', {
        ts: Date.now(),
        cmd,
        decodeType: msg.decodeType,
        audioType: msg.audioType
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
    if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
      if (this.uiNavHintActive) {
        this.uiNavHintActive = false
        this.emitAttention('nav', false)
      }
    }
    if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
      if (!this.uiNavHintActive) {
        this.uiNavHintActive = true
        this.emitAttention('nav', true)
      }
    }
  }

  private stopAllPlayers() {
    for (const player of this.players.values()) {
      try {
        player.stop()
      } catch {
        // ignore
      }
    }
    this.players.clear()
    this.primed.clear()
  }

  private stopPlayerByKey(key: PlayerKey | null) {
    if (!key) return
    const player = this.players.get(key)
    if (!player) return
    try {
      player.stop()
    } catch {
      // ignore
    }
    this.players.delete(key)
    this.primed.delete(key)
  }

  private createPlayer(
    key: PlayerKey,
    audioType: number,
    sampleRate: number,
    channels: number,
    tag?: string
  ): HostAudioOutput {
    const player: HostAudioOutput = new HostAudioOutput({
      sampleRate,
      channels,
      device: this.getConfig().audioOutputDevice || undefined,
      onOpened: (streamId) => {
        for (const cb of this.hostOutputListeners) cb(audioType, streamId, tag)
        // The stream opens at the current level instead of the host default.
        player.setVolume(this.levelForType(audioType), 0)
      }
    })
    this.outputAudioTypes.set(player, audioType)
    if (tag) this.outputTags.set(player, tag)
    player.start()
    this.players.set(key, player)
    return player
  }

  /** Opens the host stream for a channel's format. Idempotent per channel and format. */
  public primeOutput(audioType: number, sampleRate: number, channels: number, tag?: string): void {
    if (!sampleRate || !channels) return
    const suffix = tag ? `:${tag}` : ''
    const key: PlayerKey = `at${audioType}:${sampleRate}:${channels}${suffix}`
    if (this.players.has(key)) return
    this.createPlayer(key, audioType, sampleRate, channels, tag)
    this.primed.set(key, { audioType, sampleRate, channels, tag })
  }

  /** Drops the streams opened for this tag, the call when it ended. */
  public dropPrimed(tag: string): void {
    for (const [key, p] of [...this.primed]) {
      if (p.tag === tag) this.stopPlayerByKey(key)
    }
  }

  /** Sets the level of the streams of this audioType, the driver's volume hook. */
  public setHostStreamVolume(audioType: number, level: number, rampMs: number): void {
    for (const player of this.players.values()) {
      if (this.outputAudioTypes.get(player) !== audioType) continue
      if (player.hostStreamId != null) player.setVolume(level, rampMs)
    }
  }

  /** The current level for an audioType, ducking included, 1 when no stream maps to it. */
  private levelForType(audioType: number): number {
    const streams = Object.keys(ProjectionAudio.AUDIO_TYPE) as LogicalStreamKey[]
    const stream = streams.find((s) => ProjectionAudio.AUDIO_TYPE[s] === audioType)
    if (!stream) return 1
    const duck = stream === 'music' ? this.duckLevel : 1
    return this.volumes[stream] * duck
  }

  /** The open host streams with their channel tag, for a driver feeding the host itself. */
  public hostOutputs(): HostOutput[] {
    const out: HostOutput[] = []
    for (const player of this.players.values()) {
      const streamId = player.hostStreamId
      const audioType = this.outputAudioTypes.get(player)
      if (streamId == null || audioType == null) continue
      out.push({ audioType, streamId, tag: this.outputTags.get(player) })
    }
    return out
  }

  public onHostOutput(cb: HostOutputListener): () => void {
    this.hostOutputListeners.add(cb)
    return () => {
      this.hostOutputListeners.delete(cb)
    }
  }

  // Called when audioOutputDevice / audioInputDevice changed in config
  public onAudioDeviceChanged(): void {
    if (DEBUG) console.debug('[ProjectionAudio] audio device changed, resetting streams')
    // The streams are reopened on the new device and announce their new ids to the driver.
    const primed = [...this.primed.values()]
    this.stopAllPlayers()
    for (const p of primed) this.primeOutput(p.audioType, p.sampleRate, p.channels, p.tag)
  }
}
