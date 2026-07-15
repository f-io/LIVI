/**
 * CpStack — the CarPlay Wi-Fi session engine (skeleton).
 *
 * Owns the RTSP-style control connection on TCP :7000 that the phone opens
 * after joining our AP and finding the Bonjour service. This skeleton stands up
 * the server, frames requests, logs the phone's request sequence, and answers
 * GetInfo (classic displays + per-HW codec). The encrypted handshake
 * (pair-verify, auth-setup) and the media streams land in later milestones.
 */

import { EventEmitter } from 'node:events'
import net from 'node:net'
import { DEBUG } from '@main/constants'
import { AudioCommand } from '@shared/types/ProjectionEnums'
import { CP_BT_SOCK_PATH } from '../CpHelperSock'
import { AudioStream, ntp64Now } from './audioStream'
import { handleAuthSetup } from './authSetup'
import { decodeBplist, encodeBplist, type PlistValue } from './bplist'
import { ControlCipher } from './controlCipher'
import { hkdfSha512 } from './crypto'
import { ALT_UUID, buildInfoPlist, MAIN_UUID } from './getInfo'
import {
  KNOB_HID_UID,
  type KnobState,
  knobReport,
  MEDIA_HID_UID,
  mediaReport,
  TELEPHONY_HID_UID,
  TOUCH_HID_UID,
  telephonyReport,
  touchReport
} from './hid'
import { IapTunnel } from './iapTunnel'
import { KeepAliveServer } from './keepAliveServer'
import type { MfiSigner } from './mfiSigner'
import { CpMicUplink } from './micUplink'
import { PairSetup } from './pairSetup'
import { PairVerify } from './pairVerify'
import { CpRtpAudioDecoder } from './rtpAudioDecoder'
import { buildResponse, parseMessages, type RtspRequest, type RtspResponse } from './rtspMessage'
import { ScreenStream } from './screenStream'
import { TimingSync } from './timingServer'
import type { CpAudioProfile, CpStackConfig, CpStreamProfile } from './types'

const STREAM_TYPE_MAIN_SCREEN = 110
const STREAM_TYPE_ALT_SCREEN = 111
const STREAM_TYPE_MAIN_AUDIO = 100
const STREAM_TYPE_ALT_AUDIO = 101
const STREAM_TYPE_MAIN_HIGH_AUDIO = 102

// The CarPlay URL rendered on the instrument-cluster (alt) screen.
const CLUSTER_MAP_URL = 'maps:/car/instrumentcluster/map'

// AAC-LC audioFormat bits (kAirPlayAudioFormat_*): 44.1k / 48k stereo.
const AAC_LC_44K_STEREO = 0x400000
const AAC_LC_48K_STEREO = 0x800000
// OPUS mono audioFormat bits: 16k / 24k / 48k (low-latency nav/alert/Siri).
const OPUS_MONO = 0x10000000 | 0x20000000 | 0x40000000
// LPCM audioFormat bit -> the rate/channels the phone chose. PCM streams only
// appear over wired CarPlay and pass straight through; the audio output resamples
// them to the configured sink rate.
const PCM_FORMAT: Record<number, { rate: number; channels: number }> = {
  0x4: { rate: 8000, channels: 1 },
  0x8: { rate: 8000, channels: 2 },
  0x10: { rate: 16000, channels: 1 },
  0x20: { rate: 16000, channels: 2 },
  0x40: { rate: 24000, channels: 1 },
  0x80: { rate: 24000, channels: 2 },
  0x100: { rate: 32000, channels: 1 },
  0x200: { rate: 32000, channels: 2 },
  0x400: { rate: 44100, channels: 1 },
  0x800: { rate: 44100, channels: 2 },
  0x4000: { rate: 48000, channels: 1 },
  0x8000: { rate: 48000, channels: 2 }
}

// Generic DataStream (used for the iAP2-over-CarPlay tunnel) and the iAP channel UUID.
const STREAM_TYPE_DATA = 130
const IAP_DATASTREAM_UUID = 'E9459FD0-BCAD-4C45-820F-1E72447EF2F2'

/** Map a CarPlay audioType category onto LIVI's lifecycle + mic uplink decodeType. */
function audioProfile(audioType: string): CpAudioProfile {
  switch (audioType) {
    case 'telephony':
      return {
        decodeType: 5,
        audioType: 2,
        startCmd: AudioCommand.AudioPhonecallStart,
        stopCmd: AudioCommand.AudioPhonecallStop,
        label: 'telephony'
      }
    case 'speechRecognition':
      return {
        decodeType: 5,
        audioType: 1,
        startCmd: AudioCommand.AudioVoiceAssistantStart,
        stopCmd: AudioCommand.AudioVoiceAssistantStop,
        label: 'speech'
      }
    case 'media': // entertainment stream (the music/media type)
      return {
        decodeType: 4,
        audioType: 3,
        startCmd: AudioCommand.AudioMediaStart,
        stopCmd: AudioCommand.AudioMediaStop,
        label: 'media'
      }
    default: // default / alert / compatibility: low-latency nav + alert prompts (Opus)
      return {
        decodeType: 4,
        audioType: 4,
        startCmd: AudioCommand.AudioNaviStart,
        stopCmd: AudioCommand.AudioNaviStop,
        label: audioType || 'nav'
      }
  }
}

const PLIST_CONTENT_TYPE = 'application/x-apple-binary-plist'
const PAIRING_CONTENT_TYPE = 'application/pairing+tlv8'
const OCTET_CONTENT_TYPE = 'application/octet-stream'

/** Per-connection handshake + session state. */
interface CpSession {
  pairSetup: PairSetup
  pairVerify: PairVerify
  /** Set once pair-verify succeeds; from then on the connection is framed + encrypted. */
  cipher: ControlCipher | null
  /** Buffer for reassembling encrypted frames before decryption. */
  encBuf: Buffer
  /** The phone's address, used to drive the timing sync and reach its ports. */
  peerHost: string
  /** Media session servers, created during SETUP, torn down with the connection. */
  timing: TimingSync | null
  keepAlive: KeepAliveServer | null
  screen: ScreenStream | null
  clusterScreen: ScreenStream | null
  clusterCodecEmitted: boolean
  /** One record per audio stream (media/alt/high), created during SETUP: its media
   *  clock (for /feedback), the UDP stream, and its gst decoder (AAC-LC/OPUS, or null
   *  for raw PCM). Keyed by CarPlay stream type so TEARDOWN can drop just one stream. */
  audioMeta: {
    type: number
    sampleRate: number
    connectionID: PlistValue
    stream: AudioStream
    decoder: { stop(): void } | null
    /** Mic uplink for a bidirectional MainAudio stream (phone gave a send port), else null. */
    uplink: CpMicUplink | null
    /** The phone's negotiated audioLatencyMs for this stream. Drives both the decoder
     *  jitter buffer and the /feedback playback-position lag so the two stay consistent. */
    playoutLatencyMs: number
  }[]
  /** iAP2-over-CarPlay tunnel + its relay connection to the Python iAP2 stack. */
  iapTunnel: IapTunnel | null
  iapRelay: net.Socket | null
  /** TCP event channel the phone connects to after session SETUP. */
  event: net.Server | null
  /** The live event connection + its cipher, used to send HID (touch) reports. */
  eventSock: net.Socket | null
  eventCipher: ControlCipher | null
  eventCseq: number
  codecEmitted: boolean
  mainStreamReady: boolean
  lastCtrlReadNs: bigint
  heartbeat: ReturnType<typeof setInterval> | null
}

export class CpStack extends EventEmitter {
  private readonly _conns = new Set<net.Socket>()
  private _configRefresh: (() => void) | null = null
  /** The session whose event connection is live, used to route outgoing touch. */
  private _active: CpSession | null = null
  /** The session that reached RECORD, so its control-connection close ends the session. */
  private _liveSession: CpSession | null = null
  /** Control socket per session, so a WiFi-drop can destroy the hung one. */
  private readonly _sessionSock = new Map<CpSession, net.Socket>()
  /** Set while stop() tears the connection down, so our own socket destroys don't self-signal. */
  private _closing = false
  private readonly _mfi: MfiSigner
  /** Throttle counter for the /feedback diagnostics log. */
  private _fbN = 0
  private _clusterWantActive = false
  private _nightMode: boolean | null = null
  /** Last Siri speech-mode state, so we emit 'speech-active' only on transitions. */
  private _speechActive = false
  /** Mic uplinks whose MainAudio stream is currently active (fed by writeMic). */
  private readonly _activeUplinks = new Set<CpMicUplink>()

  constructor(private readonly cfg: CpStackConfig) {
    super()
    this._mfi = cfg.mfi
  }

  stop(): void {
    this._closing = true
    // Tear down live sessions (closes the iAP2-over-CarPlay tunnel to the helper,
    // so its session count drops and the carkit watcher can reconnect on replug).
    for (const sock of [...this._conns]) sock.destroy()
    this._conns.clear()
  }

  /** Adopt an already-accepted control connection (CpManager owns the :7000 listener). */
  attachSocket(sock: net.Socket): void {
    const peer = `${sock.remoteAddress}:${sock.remotePort}`
    console.log(`[cpStack] control connection from ${peer}`)
    this._conns.add(sock)
    const session: CpSession = {
      pairSetup: new PairSetup(),
      pairVerify: new PairVerify(),
      cipher: null,
      encBuf: Buffer.alloc(0),
      peerHost: sock.remoteAddress ?? '',
      timing: null,
      keepAlive: null,
      screen: null,
      clusterScreen: null,
      clusterCodecEmitted: false,
      audioMeta: [],
      iapTunnel: null,
      iapRelay: null,
      event: null,
      eventSock: null,
      eventCipher: null,
      eventCseq: 0,
      codecEmitted: false,
      mainStreamReady: false,
      lastCtrlReadNs: process.hrtime.bigint(),
      heartbeat: null
    }
    this._sessionSock.set(session, sock)
    // Handlers are async (auth-setup hits the MFi helper), so serialise the
    // per-connection processing to keep the request order and the cipher state sane.
    let acc = Buffer.alloc(0)
    let chain: Promise<void> = Promise.resolve()

    sock.on('data', (chunk: Buffer) => {
      session.lastCtrlReadNs = process.hrtime.bigint()
      chain = chain.then(async () => {
        // Once verified, incoming bytes are encrypted frames; decrypt to plaintext RTSP.
        if (session.cipher) {
          session.encBuf = Buffer.concat([session.encBuf, chunk])
          const { data, rest } = session.cipher.decrypt(session.encBuf)
          session.encBuf = rest
          acc = Buffer.concat([acc, data])
        } else {
          acc = Buffer.concat([acc, chunk])
        }

        const { messages, rest } = parseMessages(acc)
        acc = Buffer.concat([rest])
        for (const req of messages) {
          let out: Buffer
          try {
            out = buildResponse(req, await this._handle(req, session))
          } catch (e) {
            console.warn(`[cpStack] handler error for ${req.method} ${req.path}:`, e)
            out = buildResponse(req, { status: 500 })
          }
          sock.write(session.cipher ? session.cipher.encrypt(out) : out)
          // pair-verify M4 is answered in plaintext; encryption starts on the next message.
          if (!session.cipher && session.pairVerify.controlKeys) {
            const k = session.pairVerify.controlKeys
            session.cipher = new ControlCipher(k.readKey, k.writeKey)
            console.log('[cpStack] control channel encrypted')
          }
        }
      })
    })
    sock.on('error', (err) => console.warn(`[cpStack] socket error ${peer}: ${err.message}`))
    sock.on('close', () => {
      console.log(`[cpStack] control connection closed ${peer}`)
      this._conns.delete(sock)
      this._sessionSock.delete(session)
      this._teardown(session)
      if (!this._closing && session === this._liveSession) {
        this._liveSession = null
        this.emit('session-ended')
      }
    })
    session.heartbeat = setInterval(() => {
      const ctrlMs = Number(process.hrtime.bigint() - session.lastCtrlReadNs) / 1e6
      const recv = session.audioMeta.map((m) => m.stream.getLastRecvSample()).join(',')
      console.log(`[cpStack hb] alive ctrlRead=${ctrlMs.toFixed(0)}ms audioRecvSample=[${recv}]`)
    }, 1000)
  }

  private _teardown(session: CpSession): void {
    if (session.heartbeat) {
      clearInterval(session.heartbeat)
      session.heartbeat = null
    }
    session.timing?.stop()
    session.keepAlive?.stop()
    session.screen?.stop()
    session.clusterScreen?.stop()
    for (const m of session.audioMeta) {
      m.stream.stop()
      m.decoder?.stop()
      if (m.uplink) {
        this._activeUplinks.delete(m.uplink)
        m.uplink.stop()
      }
    }
    if (this._activeUplinks.size === 0) this.emit('mic-active', false, 0)
    session.iapTunnel?.stop()
    session.iapRelay?.destroy()
    session.audioMeta = []
    session.iapTunnel = null
    session.iapRelay = null
    session.event?.close()
    session.timing = null
    session.keepAlive = null
    session.screen = null
    session.clusterScreen = null
    session.event = null
  }

  /** The connection's stable pair-verify controller id (the CP device identity). */
  get activeControllerId(): string | null {
    return this._liveSession?.pairVerify.controllerId ?? null
  }

  /** Feed captured S16LE mic PCM to every active MainAudio uplink. */
  writeMic(pcm: Buffer): void {
    for (const u of this._activeUplinks) u.write(pcm)
  }

  private _normHost(h: string): string {
    return h ? h.replace(/%.*$/, '').replace(/^::ffff:/i, '') : ''
  }

  /** TEARDOWN names the streams to drop (e.g. only music when a track ends); tear down
   *  just those and leave the rest (nav, screen, timing) running. A TEARDOWN with no
   *  stream list is a session-level teardown and takes the whole session down. */
  private _handleTeardown(req: RtspRequest, session: CpSession): RtspResponse {
    let types: number[] = []
    try {
      const body = decodeBplist(req.body) as Record<string, PlistValue>
      const streams = body?.streams
      if (Array.isArray(streams)) {
        types = streams.map((s) => Number((s as Record<string, PlistValue>).type))
      }
    } catch {
      /* no/invalid body -> fall through to a full session teardown */
    }
    if (types.length === 0) {
      console.log('[cpStack] TEARDOWN (session)')
      this._teardown(session)
      return { status: 200 }
    }
    console.log(`[cpStack] TEARDOWN streams ${types.join(',')}`)
    for (const type of types) {
      if (type === STREAM_TYPE_MAIN_SCREEN) {
        session.screen?.stop()
        session.screen = null
        continue
      }
      const idx = session.audioMeta.findIndex((m) => m.type === type)
      if (idx < 0) continue
      const [m] = session.audioMeta.splice(idx, 1)
      m.stream.stop()
      m.decoder?.stop()
      if (m.uplink) {
        this._activeUplinks.delete(m.uplink)
        m.uplink.stop()
        if (this._activeUplinks.size === 0) this.emit('mic-active', false, 0)
      }
    }
    return { status: 200 }
  }

  private async _handle(req: RtspRequest, session: CpSession): Promise<RtspResponse> {
    // /feedback and OPTIONS are keepalive chatter (once a second); suppressed
    // from the log too. Everything else is worth a line.
    const chatty = req.method === 'OPTIONS' || req.path.toLowerCase().endsWith('/feedback')
    if (!chatty) {
      console.log(`[cpStack] < ${req.method} ${req.path} (body ${req.body.length}B)`)
      if (DEBUG && req.body.length > 0) {
        try {
          console.log('[cpStack]   body:', JSON.stringify(decodeBplist(req.body)))
        } catch {
          /* not a plist */
        }
      }
    }

    if (req.method === 'SETUP') {
      return await this._handleSetup(req, session)
    }
    if (req.method === 'RECORD') {
      console.log('[cpStack] RECORD (session started)')
      this._liveSession = session
      // Event commands are only valid once the session has started (older iOS stalls
      // the bring-up ~5s on a POST /command sent before RECORD). Push the initial
      // night mode now, not on event-channel connect.
      if (this._nightMode !== null) {
        this._sendEventCommand(
          session,
          encodeBplist({ type: 'setNightMode', params: { nightMode: this._nightMode } })
        )
      }
      this.emit('session-active', this._normHost(session.peerHost))
      this._openIapMessageRelay(session)
      return { status: 200 }
    }
    if (req.method === 'TEARDOWN') {
      return this._handleTeardown(req, session)
    }

    const path = req.path.toLowerCase()

    if (path.endsWith('/pair-setup')) {
      const body = session.pairSetup.handle(req.body)
      return { headers: { 'Content-Type': PAIRING_CONTENT_TYPE }, body }
    }

    if (path.endsWith('/pair-verify')) {
      const body = session.pairVerify.handle(req.body)
      return { headers: { 'Content-Type': PAIRING_CONTENT_TYPE }, body }
    }

    if (path.endsWith('/auth-setup')) {
      const body = await handleAuthSetup(req.body, this._mfi)
      if (!body) return { status: 400 }
      return { headers: { 'Content-Type': OCTET_CONTENT_TYPE }, body }
    }

    if (path.endsWith('/info')) {
      this._configRefresh?.()
      const info = buildInfoPlist(this.cfg)
      const i = info as Record<string, unknown>
      console.log(
        `[cpStack] /info audio: disableAudioOutput=${this.cfg.disableAudioOutput} audioFormats=${Array.isArray(i.audioFormats) ? i.audioFormats.length : 'MISSING'} audioLatencies=${Array.isArray(i.audioLatencies) ? i.audioLatencies.length : 'MISSING'}`
      )
      const body = encodeBplist(info)
      return { headers: { 'Content-Type': PLIST_CONTENT_TYPE }, body }
    }

    if (req.method === 'POST' && path.endsWith('/command')) {
      return this._handleCommand(req, session)
    }

    if (req.method === 'POST' && path.endsWith('/feedback')) {
      return this._buildFeedback(session)
    }

    // Everything else is simply acknowledged with a bare 200.
    if (DEBUG) console.log(`[cpStack] 200 ${req.method} ${req.path}`)
    return { status: 200 }
  }

  /** Control-channel commands from the phone (POST /command). */
  /** Track the Siri speech mode from modesChanged appStates (appStateID 1). speechMode
   *  is Recognizing(2)/Speaking(1) while Siri is active and None(-1) once it is done  */
  private _handleModesChanged(body: Record<string, PlistValue>): void {
    const params = (body.params ?? {}) as Record<string, PlistValue>
    const appStates = params.appStates
    if (!Array.isArray(appStates)) return
    let active = this._speechActive
    for (const s of appStates) {
      const st = (s ?? {}) as Record<string, PlistValue>
      if (Number(st.appStateID) !== 1 || st.speechMode === undefined) continue
      const mode = Number(st.speechMode)
      active = mode === 1 || mode === 2
    }
    if (active !== this._speechActive) {
      this._speechActive = active
      console.log(`[cpStack] Siri speech ${active ? 'active' : 'done'}`)
      this.emit('speech-active', active)
    }
  }

  private _handleCommand(req: RtspRequest, session: CpSession): RtspResponse {
    let body: Record<string, PlistValue> = {}
    try {
      body = (decodeBplist(req.body) ?? {}) as Record<string, PlistValue>
    } catch {
      /* empty or non-plist body: nothing to route */
    }
    const type = String(body.type ?? '')
    // The car/home button in the CarPlay dock sends requestUI (no url), asking us
    // to bring up the head-unit's own UI. Mirror AA's host-ui-requested path.
    if (type === 'requestUI') {
      console.log('[cpStack] requestUI → host UI requested')
      this.emit('host-ui-requested')
    } else if (type === 'disableBluetooth') {
      const params = (body.params ?? {}) as Record<string, PlistValue>
      const deviceID = String(params.deviceID ?? '')
      console.log(`[cpStack] disableBluetooth (deviceID=${deviceID}) — disconnecting BT`)
      this.emit('disable-bluetooth', deviceID)
    } else if (type === 'modesChanged') {
      this._handleModesChanged(body)
      if (DEBUG) {
        // Log who currently owns each resource so we can see if the phone granted us
        // main audio (resourceID 2, entity: 1 = device/controller, 2 = accessory).
        const j = JSON.stringify(body.params ?? body, (_k, v) =>
          typeof v === 'bigint' ? Number(v) : v
        )
        console.log(`[cpStack] modesChanged ${j}`)
      }
    } else if (type === 'iAPSendMessage') {
      const p = (body.params ?? {}) as Record<string, PlistValue>
      const d = p.data
      if (Buffer.isBuffer(d)) {
        session.iapRelay?.write(d)
      }
    } else if (type && DEBUG) {
      console.log(`[cpStack] command '${type}' (ack 200)`)
    }
    return { status: 200 }
  }

  /**
   * Answer POST /feedback with the media-clock stream list. The buffered media
   * stream (type 102) is clock-driven: an empty response makes the phone
   * treat the stream as dead and tear it down every few seconds. We report
   * {type, sampleRate} per active audio stream here (the full timestamp anchor is
   * only added once a playback-rate estimate exists, which we do not track yet).
   */
  private _buildFeedback(session: CpSession): RtspResponse {
    if (session.audioMeta.length === 0) return { status: 200 }
    const streams = session.audioMeta.map((m) => {
      const s: Record<string, PlistValue> = { type: m.type, sampleRate: m.sampleRate }
      // Report the extrapolated PLAYBACK position: origin sample + elapsed real time
      // times the rate. It advances at real time (sink hardware clock) no matter how
      // fast the phone sends, which forces it to keep feeding at real time.
      const o = m.stream.getOrigin()
      if (o) {
        const nowNs = process.hrtime.bigint()
        // Playback starts only after the output buffer fills.
        // Use this stream's negotiated audioLatencyMs so the
        // reported lag matches the jitter-buffer depth exactly (buffered stream = 1000ms).
        const PLAYOUT_LATENCY_SEC = m.playoutLatencyMs / 1000
        const elapsedSec = Math.max(0, Number(nowNs - o.originNs) / 1e9 - PLAYOUT_LATENCY_SEC)
        const play = (o.firstSample + Math.round(elapsedSec * m.sampleRate)) >>> 0
        s.streamConnectionID = m.connectionID
        // hostTime must be in the phone's synchronized clock domain (via TimingSync),
        // else the phone can't place our media-clock anchor. hostTimeRaw stays raw.
        s.timestamp = session.timing ? session.timing.syncedNtp() : ntp64Now()
        s.timestampRawNs = nowNs
        s.sampleTime = play
        if (DEBUG) {
          // buffer = received-so-far − reported-playback (samples). Positive = phone
          // is ahead of our playback; shrinking toward 0/negative = feed falling behind.
          const buf = (m.stream.getLastRecvSample() - play) | 0
          this._fbN++
          if (this._fbN % 3 === 0) {
            console.log(
              `[cpStack fb] type=${m.type} play=${play} recv=${m.stream.getLastRecvSample()} buffer=${buf} (${(buf / m.sampleRate).toFixed(2)}s)`
            )
          }
        }
      }
      return s
    })
    return { headers: { 'Content-Type': PLIST_CONTENT_TYPE }, body: encodeBplist({ streams }) }
  }

  private async _handleSetup(req: RtspRequest, session: CpSession): Promise<RtspResponse> {
    let body: PlistValue
    try {
      body = decodeBplist(req.body)
    } catch (e) {
      console.warn('[cpStack] SETUP body is not a plist:', (e as Error).message)
      return { status: 400 }
    }
    const dict = (body ?? {}) as Record<string, PlistValue>
    const streams = dict.streams

    if (Array.isArray(streams)) {
      const respStreams: PlistValue[] = []
      for (const s of streams) {
        const sd = s as Record<string, PlistValue>
        const type = Number(sd.type)
        if (type === STREAM_TYPE_MAIN_SCREEN) {
          respStreams.push({ type, dataPort: await this._setupScreen(sd, session) })
        } else if (type === STREAM_TYPE_ALT_SCREEN) {
          respStreams.push({ type, dataPort: await this._setupScreen(sd, session, true) })
        } else if (
          type === STREAM_TYPE_MAIN_AUDIO ||
          type === STREAM_TYPE_ALT_AUDIO ||
          type === STREAM_TYPE_MAIN_HIGH_AUDIO
        ) {
          console.log(`[cpStack] SETUP audio stream type ${type}`)
          respStreams.push(await this._setupAudio(sd, session, type))
        } else if (type === STREAM_TYPE_DATA) {
          const resp = await this._setupDataStream(sd, session)
          if (resp) respStreams.push(resp)
        } else {
          console.log(`[cpStack]   SETUP stream type ${type} not handled yet`)
        }
      }
      return {
        headers: { 'Content-Type': PLIST_CONTENT_TYPE },
        body: encodeBplist({ streams: respStreams })
      }
    }

    // Session-level SETUP carries the phone's identity, authoritative over any
    // transport: deviceID = its BT MAC, macAddress = its WiFi MAC on our AP, plus
    // the friendly name and model.
    const idName = typeof dict.name === 'string' ? dict.name : ''
    const idDevice = typeof dict.deviceID === 'string' ? dict.deviceID : ''
    const idWifi = typeof dict.macAddress === 'string' ? dict.macAddress.toLowerCase() : ''
    const idModel = typeof dict.model === 'string' ? dict.model : ''
    if (idName || idDevice || idWifi) {
      this.emit('device-info', {
        name: idName,
        deviceId: idDevice,
        wifiMac: idWifi,
        model: idModel
      })
    }

    // Session-level SETUP: advertise our timing and event ports. The phone opens
    // a TCP connection to the event port before it sends the stream-level SETUP.
    const timing = new TimingSync()
    const timingPort = await timing.listen()
    session.timing = timing
    // Drive the NTP clock sync against the phone's timing port, or the phone
    // tears the session down after a few seconds.
    const peerTimingPort = Number(dict.timingPort) || 0
    if (peerTimingPort > 0) timing.start(session.peerHost, peerTimingPort)
    const eventPort = await this._openEventChannel(session)
    let keepAlivePort = 0
    if (dict.keepAliveLowPower) {
      const keepAlive = new KeepAliveServer()
      session.keepAlive = keepAlive
      keepAlivePort = await keepAlive.listen()
    }
    console.log(
      `[cpStack] SETUP session (timingPort=${timingPort}, eventPort=${eventPort}, keepAlivePort=${keepAlivePort})`
    )
    const resp: { [k: string]: PlistValue } = { timingPort, eventPort }
    if (keepAlivePort) resp.keepAlivePort = keepAlivePort
    const feats: PlistValue[] = this.cfg.hevc ? ['hevc', 'iAPChannel'] : ['iAPChannel']
    feats.push('viewAreas')
    if (this.cfg.cluster) feats.push('altScreen')
    resp.enabledFeatures = feats
    return { headers: { 'Content-Type': PLIST_CONTENT_TYPE }, body: encodeBplist(resp) }
  }

  private _openEventChannel(session: CpSession): Promise<number> {
    return new Promise((resolve, reject) => {
      const server = net.createServer((s) => {
        console.log('[cpStack] event channel connected')
        // The event connection is encrypted from the first byte with Events keys
        // derived from the pair-verify secret. We use it to push HID (touch) reports.
        const shared = session.pairVerify.sharedSecret
        if (shared) {
          // Unlike the control channel, the event transport is NOT key-swapped:
          // the accessory writes with Events-Write and reads with Events-Read.
          const writeKey = hkdfSha512(shared, 'Events-Salt', 'Events-Write-Encryption-Key', 32)
          const readKey = hkdfSha512(shared, 'Events-Salt', 'Events-Read-Encryption-Key', 32)
          session.eventCipher = new ControlCipher(readKey, writeKey)
        }
        session.eventSock = s
        this._active = session
        let enc: Buffer = Buffer.alloc(0)
        let plain: Buffer = Buffer.alloc(0)
        s.on('data', (chunk: Buffer) => {
          const cipher = session.eventCipher
          if (!cipher) return
          enc = Buffer.concat([enc, chunk])
          try {
            const { data, rest } = cipher.decrypt(enc)
            enc = rest
            if (data.length) plain = Buffer.concat([plain, data])
          } catch (err) {
            console.warn(`[cpStack] event channel decrypt failed: ${(err as Error).message}`)
            return
          }
          const { messages, rest } = parseMessages(plain)
          plain = rest
          for (const m of messages) this._onEventMessage(session, m)
        })
        s.on('error', () => {})
        s.on('close', () => {
          console.log('[cpStack] event channel closed')
          session.eventSock = null
          if (this._active === session) this._active = null
        })
      })
      server.on('error', reject)
      server.listen({ port: 0, host: '::', ipv6Only: false }, () => {
        session.event = server
        const addr = server.address()
        resolve(typeof addr === 'object' && addr ? addr.port : 0)
      })
    })
  }

  /** Send touch contacts (normalised 0..1) to the phone as a multitouch HID report. */
  sendTouches(contacts: { x: number; y: number; down: boolean; id: number }[]): void {
    const s = this._active
    if (!s) return
    const report = touchReport(
      contacts.map((c) => ({
        id: c.id,
        x: c.x * this.cfg.main.widthPixels,
        y: c.y * this.cfg.main.heightPixels,
        down: c.down
      }))
    )
    this._sendEventCommand(
      s,
      encodeBplist({ type: 'hidSendReport', uuid: TOUCH_HID_UID.toString(16), hidReport: report })
    )
  }

  private _sendHidReport(uid: number, report: Buffer): void {
    const s = this._active
    if (!s) return
    this._sendEventCommand(
      s,
      encodeBplist({ type: 'hidSendReport', uuid: uid.toString(16), hidReport: report })
    )
  }

  /** Momentary knob event: send the pressed state, then a released (all-zero) report. */
  sendKnob(state: KnobState, momentary = true): void {
    this._sendHidReport(KNOB_HID_UID, knobReport(state))
    if (momentary) this._sendHidReport(KNOB_HID_UID, knobReport({}))
  }

  /** Hold or release the knob select button (no auto-release). */
  sendKnobSelect(down: boolean): void {
    this._sendHidReport(KNOB_HID_UID, knobReport({ select: down }))
  }

  /** Momentary media key: send the index, then release (0). */
  sendMedia(index: number): void {
    this._sendHidReport(MEDIA_HID_UID, mediaReport(index))
    this._sendHidReport(MEDIA_HID_UID, mediaReport(0))
  }

  /** Momentary telephony key: send the index, then release (0). */
  sendTelephony(index: number): void {
    this._sendHidReport(TELEPHONY_HID_UID, telephonyReport(index))
    this._sendHidReport(TELEPHONY_HID_UID, telephonyReport(0))
  }

  /** Invoke Siri as a dedicated Siri button (R6 3.3.7.1.2): buttonDown(2) then buttonUp(3).
   *  Sent as an immediate momentary click, not tied to the physical hold, so the phone
   *  sees a tap and starts a conversational session (listens via VAD and replies) rather
   *  than push-to-talk that submits on release before the user has spoken. */
  invokeSiri(): void {
    const s = this._active
    if (!s) {
      console.log('[cpStack] invokeSiri: no active event connection, ignoring')
      return
    }
    console.log('[cpStack] invokeSiri: requestSiri buttonDown+buttonUp (click)')
    this._sendEventCommand(s, encodeBplist({ type: 'requestSiri', params: { siriAction: 2 } }))
    this._sendEventCommand(s, encodeBplist({ type: 'requestSiri', params: { siriAction: 3 } }))
  }

  /** Send a bplist command to the phone over the encrypted event channel. */
  /** The event channel is bidirectional reverse-HTTP: the phone both answers our
   *  commands and sends its own requests, which MUST get a response (older iOS
   *  blocks session bring-up on 5s request timeouts otherwise). */
  private _onEventMessage(session: CpSession, msg: RtspRequest): void {
    if (msg.method.startsWith('RTSP/') || msg.method.startsWith('HTTP/')) {
      if (msg.path !== '200')
        console.warn(`[cpStack] event response ${msg.path} ${msg.protocol ?? ''}`)
      return
    }
    let decoded = ''
    if (msg.body.length) {
      try {
        decoded = ` ${JSON.stringify(decodeBplist(msg.body))}`
      } catch {
        decoded = ` (${msg.body.length}B non-plist body)`
      }
    }
    console.log(`[cpStack] event < ${msg.method} ${msg.path}${decoded}`)
    if (!session.eventSock || !session.eventCipher) return
    session.eventSock.write(session.eventCipher.encrypt(buildResponse(msg, { status: 200 })))
  }

  private _sendEventCommand(s: CpSession, body: Buffer): void {
    if (!s.eventSock || !s.eventCipher) return
    s.eventCseq++
    const head =
      `POST /command RTSP/1.0\r\n` +
      `Content-Type: ${PLIST_CONTENT_TYPE}\r\n` +
      `Content-Length: ${body.length}\r\n` +
      `CSeq: ${s.eventCseq}\r\n\r\n`
    const msg = Buffer.concat([Buffer.from(head, 'utf8'), body])
    s.eventSock.write(s.eventCipher.encrypt(msg))
  }

  private async _setupAudio(
    sd: Record<string, PlistValue>,
    session: CpSession,
    type: number
  ): Promise<PlistValue> {
    const shared = session.pairVerify.sharedSecret
    if (!shared) throw new Error('audio SETUP arrived before pair-verify')
    const streamId = sd.streamConnectionID
    // The phone tells us how far ahead it buffers via audioLatencyMs (1000 for the
    // buffered music stream). We size our jitter buffer to
    // exactly that so it never underruns, and report the same lag in /feedback. Falls
    // back to 1000 for buffered / 0 for low-latency if the field is absent.
    const audioLatencyMs = Number(sd.audioLatencyMs) || 0
    // Same DataStream key derivation as the screen: HKDF-SHA512(pair-verify shared,
    // "DataStream-Salt"<id>, "DataStream-Output-Encryption-Key"). Audio output streams
    // (phone -> us) are sealed with the output key.
    const key = hkdfSha512(
      shared,
      `DataStream-Salt${streamId}`,
      'DataStream-Output-Encryption-Key',
      32
    )
    const prof = audioProfile(String(sd.audioType ?? 'media'))
    // The phone echoes the single chosen format. AAC-LC (0x400000 44.1k / 0x800000
    // 48k stereo) is decoded to PCM by a gst child; PCM formats pass straight through.
    const fmt = Number(sd.audioFormat) || 0
    const isAacLc = (fmt & (AAC_LC_44K_STEREO | AAC_LC_48K_STEREO)) !== 0
    const isOpus = (fmt & OPUS_MONO) !== 0
    const aacIs48k = (fmt & AAC_LC_48K_STEREO) !== 0
    // OPUS decodes to 48k mono, AAC-LC to 44.1k/48k stereo, PCM passes through at
    // whatever rate/channels the phone chose. The audio output resamples any of
    // these to our configured output rate (44.1k or 48k).
    const pcm = PCM_FORMAT[fmt]
    let sampleRate: number
    let channels: number
    if (isOpus) {
      sampleRate = 48000
      channels = 1
    } else if (isAacLc) {
      sampleRate = aacIs48k ? 48000 : 44100
      channels = 2
    } else if (pcm) {
      sampleRate = pcm.rate
      channels = pcm.channels
    } else {
      sampleRate = 44100
      channels = 2
    }
    const streamProf: CpStreamProfile = { ...prof, sampleRate, channels }
    const stream = new AudioStream(key, prof.label)
    let decoder: { stop(): void } | null = null
    if (isAacLc || isOpus) {
      // Compressed audio (AAC-LC/OPUS) over RTP: feed the reconstructed RTP into a
      // udpsrc/rtpjitterbuffer/depay/decode pipeline. The jitter buffer paces the
      // phone's bursty delivery back to steady real time (the payload type is the
      // CarPlay stream type; AAC clocks at its sample rate, OPUS always at 48k).
      const dec = new CpRtpAudioDecoder({
        codec: isAacLc ? 'aac-lc' : 'opus',
        payloadType: type,
        clockRate: isOpus ? 48000 : sampleRate,
        channels,
        latencyMs: audioLatencyMs > 0 ? audioLatencyMs : 1000,
        label: prof.label
      })
      dec.on('pcm', (pcm: Buffer) => this.emit('audio-frame', pcm, streamProf))
      await dec.start()
      decoder = dec
      stream.on('rtp', (rtp: Buffer) => dec.write(rtp))
    } else {
      // LPCM passthrough: the wire samples are 16-bit big-endian, swap to S16LE
      // (guard the odd-length case so swap16 can't throw in the packet handler).
      stream.on('pcm', (pcm: Buffer) =>
        this.emit('audio-frame', pcm.length % 2 === 0 ? pcm.swap16() : pcm, streamProf)
      )
    }
    // MainAudio is bidirectional: a send port in the phone's request means it wants
    // mic. Derive the input key (mirror of the output key), build the uplink, and
    // bracket mic capture on this stream's active/stop. Over wireless the phone picks
    // OPUS for the mic (PCM is USB-only), so the uplink encodes; the encode/capture rate
    // is the negotiated OPUS rate (16k/24k/48k), independent of the downlink 48k.
    let uplink: CpMicUplink | null = null
    const phoneMicPort = type === STREAM_TYPE_MAIN_AUDIO ? Number(sd.dataPort) || 0 : 0
    // Frame duration from the phone's framesPerPacket (falls back to 20ms).
    const opusRate =
      fmt & 0x40000000 ? 48000 : fmt & 0x20000000 ? 24000 : fmt & 0x10000000 ? 16000 : 24000
    const micRate = isOpus ? opusRate : sampleRate
    const micChannels = isOpus ? 1 : channels
    const framesPerPacket = Number(sd.framesPerPacket) || 0
    const frameMs = framesPerPacket > 0 ? Math.round((framesPerPacket / micRate) * 1000) : 20
    // OPUS low-latency bitrate tiers (R6): 48k ≤24kHz, 64k ≤32kHz, 96k at 48kHz.
    const bitrate = micRate <= 24000 ? 48000 : micRate <= 32000 ? 64000 : 96000
    if (phoneMicPort > 0) {
      const inputKey = hkdfSha512(
        shared,
        `DataStream-Salt${streamId}`,
        'DataStream-Input-Encryption-Key',
        32
      )
      uplink = new CpMicUplink({
        key: inputKey,
        host: session.peerHost,
        port: phoneMicPort,
        sampleRate: micRate,
        channels: micChannels,
        payloadType: type,
        codec: isOpus ? 'opus' : 'pcm',
        frameMs,
        bitrate,
        label: prof.label
      })
    }
    stream.on('active', (active: boolean) => {
      this.emit('audio-active', prof, active)
      if (!uplink) return
      if (active) {
        uplink.start()
        this._activeUplinks.add(uplink)
        this.emit('mic-active', true, micRate, micChannels)
      } else {
        uplink.stop()
        this._activeUplinks.delete(uplink)
        if (this._activeUplinks.size === 0) this.emit('mic-active', false, 0, 0)
      }
    })
    // /feedback reports this stream's live media-clock anchor (its latest reception
    // position + time) so the phone keeps the buffered stream fed.
    session.audioMeta.push({
      type,
      sampleRate,
      connectionID: streamId as PlistValue,
      playoutLatencyMs: audioLatencyMs,
      stream,
      decoder,
      uplink
    })
    const { dataPort, controlPort } = await stream.listen()
    console.log(
      `[cpStack] SETUP audio (type ${type}, audioType=${sd.audioType}, format=0x${fmt.toString(16)}, codec=${isAacLc ? 'aac-lc (gst decode)' : isOpus ? 'opus (gst decode)' : 'pcm'}, audioLatencyMs=${audioLatencyMs}, dataPort=${dataPort}, controlPort=${controlPort}, id=${streamId})`
    )
    // Echo the phone's streamConnectionID back: without it the phone
    // can't correlate our response to its request and tears the stream back down.
    return { type, dataPort, controlPort, streamConnectionID: streamId as PlistValue }
  }

  private async _setupDataStream(
    sd: Record<string, PlistValue>,
    session: CpSession
  ): Promise<PlistValue | null> {
    const uuid = String(sd.clientTypeUUID ?? '').toUpperCase()
    if (uuid !== IAP_DATASTREAM_UUID) {
      console.log(`[cpStack]   DataStream ${uuid || '(no uuid)'} not handled yet`)
      return null
    }
    const shared = session.pairVerify.sharedSecret
    if (!shared) throw new Error('iAP DataStream SETUP arrived before pair-verify')
    const seed = sd.seed as bigint | number
    const tunnel = new IapTunnel(shared, seed)
    tunnel.on('iap', (iap: Buffer) => session.iapRelay?.write(iap))
    const dataPort = await tunnel.listen()
    session.iapTunnel = tunnel
    console.log(`[cpStack] SETUP iAP tunnel (type 130, dataPort=${dataPort}, seed=${seed})`)
    return { type: STREAM_TYPE_DATA, streamID: 1, dataPort }
  }

  private _openIapMessageRelay(session: CpSession): void {
    if (session.iapRelay) return
    const sock = net.createConnection(CP_BT_SOCK_PATH)
    session.iapRelay = sock
    sock.write(`tunnel ${session.pairVerify.controllerId ?? ''}\n`)
    sock.on('data', (d: Buffer) => this._sendIapMessage(session, d))
    sock.on('error', (e) => console.warn(`[cpStack] iAP relay error: ${e.message}`))
    sock.on('close', () => {
      if (session.iapRelay === sock) session.iapRelay = null
    })
  }

  private _sendIapMessage(session: CpSession, data: Buffer): void {
    this._sendEventCommand(session, encodeBplist({ type: 'iAPSendMessage', params: { data } }))
  }

  private async _setupScreen(
    sd: Record<string, PlistValue>,
    session: CpSession,
    isCluster = false
  ): Promise<number> {
    const shared = session.pairVerify.sharedSecret
    if (!shared) throw new Error('screen SETUP arrived before pair-verify')
    const streamId = sd.streamConnectionID
    // key = HKDF-SHA512(pair-verify shared, "DataStream-Salt"<id>, "DataStream-Output-Encryption-Key").
    const key = hkdfSha512(
      shared,
      `DataStream-Salt${streamId}`,
      'DataStream-Output-Encryption-Key',
      32
    )
    const codec = this.cfg.hevc ? 'h265' : 'h264'
    const screen = new ScreenStream(codec, key)
    const codecEvent = isCluster ? 'cluster-video-codec' : 'video-codec'
    const frameEvent = isCluster ? 'cluster-video-frame' : 'video-frame'
    let firstFrame = true
    // The stream announces its real codec from the config atom; emit it once,
    // before any frame, so gst-host builds the matching pipeline.
    screen.on('codec', (c: 'h264' | 'h265') => {
      if (isCluster) {
        if (!session.clusterCodecEmitted) {
          this.emit(codecEvent, c)
          session.clusterCodecEmitted = true
        }
      } else if (!session.codecEmitted) {
        this.emit(codecEvent, c)
        session.codecEmitted = true
      }
    })
    const forward = (annexB: Buffer): void => {
      if (firstFrame) {
        firstFrame = false
        if (!isCluster && !session.mainStreamReady) {
          session.mainStreamReady = true
          if (this._clusterWantActive) this._activateClusterStream(session)
        }
      }
      this.emit(frameEvent, annexB)
    }
    screen.on('config', forward)
    screen.on('frame', forward)
    const port = await screen.listen()
    if (isCluster) session.clusterScreen = screen
    else session.screen = screen
    console.log(
      `[cpStack] SETUP screen (type ${isCluster ? 111 : 110}, dataPort=${port}, codec=${codec}, id=${streamId})`
    )
    return port
  }

  /** Tell the phone to switch its CarPlay UI between day and night appearance. */
  setNightMode(night: boolean): void {
    this._nightMode = night
    const s = this._active
    if (s) {
      this._sendEventCommand(
        s,
        encodeBplist({ type: 'setNightMode', params: { nightMode: night } })
      )
    }
  }

  forceMainKeyframe(): void {
    const s = this._active
    if (!s || !s.mainStreamReady) return
    this._sendEventCommand(s, encodeBplist({ type: 'forceKeyFrame', params: { uuid: MAIN_UUID } }))
  }

  forceClusterKeyframe(): void {
    const s = this._active
    if (!s || !s.mainStreamReady || !this._clusterWantActive) return
    this._sendEventCommand(s, encodeBplist({ type: 'forceKeyFrame', params: { uuid: ALT_UUID } }))
  }

  applyDisplayConfig(next: CpStackConfig): void {
    Object.assign(this.cfg, next)
  }

  setConfigRefresh(fn: () => void): void {
    this._configRefresh = fn
  }

  /** Ask the phone to render / stop the instrument-cluster map on the alt screen. */
  setClusterStreamActive(active: boolean): void {
    if (!this.cfg.cluster) return
    this._clusterWantActive = active
    const s = this._active
    if (!s || !s.mainStreamReady) return
    if (active) this._activateClusterStream(s)
    else this._sendEventCommand(s, encodeBplist({ type: 'stopUI', params: { uuid: ALT_UUID } }))
  }

  private _activateClusterStream(s: CpSession): void {
    this._sendEventCommand(
      s,
      encodeBplist({ type: 'showUI', params: { uuid: ALT_UUID, url: CLUSTER_MAP_URL } })
    )
    this._sendEventCommand(s, encodeBplist({ type: 'forceKeyFrame', params: { uuid: ALT_UUID } }))
  }
}
