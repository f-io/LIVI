/**
 * Control channel (channel 0) message handler.
 *
 * Handles all control-plane messages: ping keepalive, service discovery,
 * channel open/close, audio focus, and shutdown.
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, CH, CTRL_MSG, FRAME_FLAGS, STATUS_OK } from '../constants.js'
import { decode, encode, type ProtoTypes } from '../proto/index.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export class ControlChannel extends EventEmitter {
  // Events emitted:
  //   'service-discovery-request'  (req: object)
  //   'channel-open-request'       (channelId: number)
  //   'av-setup-request'           (channelId: number, payload: Buffer)
  //   'ping'                       (timestamp: number)
  //   'audio-focus-request'        (req: object)
  //   'shutdown'                   (reason: number)

  constructor(
    private readonly _proto: ProtoTypes,
    private readonly _send: SendFn
  ) {
    super()
  }

  handleMessage(msgId: number, payload: Buffer): void {
    switch (msgId) {
      case CTRL_MSG.SERVICE_DISCOVERY_REQUEST:
        this._onServiceDiscoveryRequest(payload)
        break

      case CTRL_MSG.CHANNEL_OPEN_RESPONSE:
        // Phone accepted our channel open — nothing to do here; setup continues automatically
        try {
          const resp = decode(this._proto.ChannelOpenResponse, payload)
          if ((resp['status'] as number) !== STATUS_OK) {
            console.warn(`[ControlChannel] ChannelOpenResponse status=${resp['status']}`)
          }
        } catch {
          /* ignore parse errors */
        }
        break

      case CTRL_MSG.CHANNEL_OPEN_REQUEST:
        // Phone requests us to open a channel (rare, but handle it).
        // aasdk ChannelOpenRequest: { priority: sint32, service_id: int32 } → protobufjs = serviceId
        try {
          const req = decode(this._proto.ChannelOpenRequest, payload)
          const chId = (req['serviceId'] ?? req['channelId'] ?? 0) as number
          this.emit('channel-open-request', chId)
        } catch {
          /* ignore */
        }
        break

      case CTRL_MSG.PING_REQUEST:
        this._onPingRequest(payload)
        break

      case CTRL_MSG.PING_RESPONSE:
        // Phone responded to our ping — just log, nothing else needed
        break

      case CTRL_MSG.AUDIO_FOCUS_REQUEST:
        this._onAudioFocusRequest(payload)
        break

      case CTRL_MSG.NAVIGATION_FOCUS_REQUEST:
        this._onNavigationFocusRequest(payload)
        break

      case CTRL_MSG.SHUTDOWN_REQUEST:
        try {
          // ShutdownRequest has a 'reason' field (int32)
          const reason = payload.length >= 2 ? payload.readUInt32BE(0) : 0
          this.emit('shutdown', reason)
        } catch {
          this.emit('shutdown', 0)
        }
        break

      case CTRL_MSG.BINDING_REQUEST:
        this._onBindingRequest(payload)
        break

      // AV setup requests arrive on control channel for some channel IDs
      case AV_MSG.SETUP_REQUEST:
        // This shouldn't arrive on channel 0, but handle defensively
        console.debug('[ControlChannel] SETUP_REQUEST on control channel — ignored')
        break

      default:
        console.debug(
          `[ControlChannel] unhandled msgId=0x${msgId.toString(16)} len=${payload.length}`
        )
    }
  }

  // Called by Session when an AV setup arrives on a service channel
  handleAVSetupRequest(channelId: number, payload: Buffer): void {
    this.emit('av-setup-request', channelId, payload)
  }

  // ── Outbound ──────────────────────────────────────────────────────────────

  sendChannelOpenResponse(_channelId: number, status: number): void {
    const buf = encode(this._proto.ChannelOpenResponse, { status })
    // Per aasdk ControlServiceChannel::sendChannelOpenResponse:
    //   sendEncrypted(messenger::Message{ CONTROL, ControlMessageType::CHANNEL_OPEN_RESPONSE, ... })
    // CHANNEL_OPEN_RESPONSE always goes on ch=0 (control channel).
    this._send(CH.CONTROL, FRAME_FLAGS.ENC_CONTROL, CTRL_MSG.CHANNEL_OPEN_RESPONSE, buf)
  }

  // ── Inbound handlers ─────────────────────────────────────────────────────

  private _onServiceDiscoveryRequest(payload: Buffer): void {
    try {
      const req = decode(this._proto.ServiceDiscoveryRequest, payload)
      // aasdk ServiceDiscoveryRequest fields (after camelCase conversion):
      //   smallIcon, mediumIcon, largeIcon (bytes), labelText, deviceName, phoneInfo
      const devName = (req['deviceName'] ?? '?') as string
      const label = (req['labelText'] ?? '?') as string
      const phoneInfo = req['phoneInfo'] as Record<string, unknown> | undefined
      console.log(
        `[ControlChannel] ServiceDiscoveryRequest device="${devName}" label="${label}" phone_info=${JSON.stringify(phoneInfo ?? {})}`
      )
      this.emit('service-discovery-request', req)
    } catch (e) {
      console.warn('[ControlChannel] failed to parse ServiceDiscoveryRequest:', e)
      this.emit('service-discovery-request', {})
    }
  }

  private _onPingRequest(payload: Buffer): void {
    try {
      const req = decode(this._proto.PingRequest, payload)
      const ts = (req['timestamp'] as number) ?? 0
      const buf = encode(this._proto.PingResponse, { timestamp: ts })
      // PING_RESPONSE is plaintext per aasdk EncryptionType for PING_RESPONSE (ch=0, msgId=0x000c)
      this._send(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.PING_RESPONSE, buf)
      this.emit('ping', ts)
    } catch (e) {
      console.warn('[ControlChannel] ping parse error:', e)
    }
  }

  private _onAudioFocusRequest(payload: Buffer): void {
    // AudioFocusRequest:      field 1 = audio_focus_type (varint, AudioFocusRequestType)
    // AudioFocusNotification: field 1 = focus_state      (varint, AudioFocusStateType)
    //
    // Manually decode/encode (no AudioFocus protos in aa-native).
    //
    //
    //   REQUEST type                      → RESPONSE state
    //   ─────────────────────────────────────────────────────
    //   GAIN (1)                          → STATE_GAIN (1)
    //   GAIN_TRANSIENT (2)                → STATE_GAIN_TRANSIENT (2)
    //   GAIN_TRANSIENT_MAY_DUCK (3)       → STATE_GAIN_TRANSIENT (2)
    //   RELEASE (4)                       → STATE_LOSS (3)
    //   unknown / 0                       → STATE_LOSS (3)
    //
    // CRITICAL: previous aa-native mapped RELEASE→GAIN (wrong direction), which
    // caused the phone to close TCP immediately after AudioFocusResponse — every
    // session looked like a "verifier", but the real cause was this bug.
    // With the corrected mapping, phone proceeds to send CHANNEL_OPEN_REQUESTs.
    try {
      // Decode: payload[0] = tag (0x08 = field1/varint), payload[1] = type value
      console.log(`[ControlChannel] AudioFocusRequest raw: ${payload.toString('hex')}`)
      let focusType = 0
      if (payload.length >= 2 && payload[0] === 0x08) {
        focusType = payload[1]!
      }

      let focusState: number
      switch (focusType) {
        case 1:
          focusState = 1
          break // GAIN                    → STATE_GAIN
        case 2:
          focusState = 2
          break // GAIN_TRANSIENT          → STATE_GAIN_TRANSIENT
        case 3:
          focusState = 2
          break // GAIN_TRANSIENT_MAY_DUCK → STATE_GAIN_TRANSIENT
        case 4:
          focusState = 3
          break // RELEASE                 → STATE_LOSS
        default:
          focusState = 3
          break // unknown                 → STATE_LOSS
      }
      const stateName =
        ({ 1: 'GAIN', 2: 'GAIN_TRANSIENT', 3: 'LOSS' } as Record<number, string>)[focusState] ?? '?'
      const typeName =
        (
          { 1: 'GAIN', 2: 'GAIN_TRANSIENT', 3: 'GAIN_TRANSIENT_MAY_DUCK', 4: 'RELEASE' } as Record<
            number,
            string
          >
        )[focusType] ?? '?'
      console.log(
        `[ControlChannel] AudioFocus type=${focusType}(${typeName}) → state=${focusState}(${stateName})`
      )

      // Encode response: field 1, varint = focusState
      const respBuf = Buffer.from([0x08, focusState])
      this._send(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.AUDIO_FOCUS_RESPONSE, respBuf)
    } catch (e) {
      console.warn('[ControlChannel] audio focus response error:', e)
    }
    this.emit('audio-focus-request', {})
  }

  private _onNavigationFocusRequest(payload: Buffer): void {
    // Auto-grant navigation focus — echo the request payload back as the response.
    // NavigationFocusResponse: same field structure as request (field 1 = type int32)
    try {
      console.log(`[ControlChannel] NavigationFocusRequest raw: ${payload.toString('hex')}`)
      this._send(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.NAVIGATION_FOCUS_RESPONSE, payload)
    } catch (e) {
      console.warn('[ControlChannel] navigation focus response error:', e)
    }
  }

  private _onBindingRequest(payload: Buffer): void {
    try {
      const req = decode(this._proto.BindingRequest, payload)
      console.debug('[ControlChannel] BindingRequest scanCodes=', req['scan_codes'])
      // Respond OK
      const buf = encode(this._proto.BindingResponse, { status: STATUS_OK })
      this._send(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.BINDING_RESPONSE, buf)
    } catch (e) {
      console.warn('[ControlChannel] binding request error:', e)
    }
  }
}
