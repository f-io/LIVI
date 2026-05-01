/**
 * Sensor channel handler (GAL type CH.SENSOR = 1).
 *
 * Sends sensor data FROM the HU TO the phone (direction: HU → phone).
 *
 * Wire protocol:
 *   HU → Phone: SENSOR_EVENT_INDICATION (0x8001) — sensor batch
 *
 * Required sensors (phone refuses to show certain UI without them):
 *   - DRIVING_STATUS (type 13): UNRESTRICTED=0, LIMIT_MESSAGE_LEN=1,
 *                                LIMIT_GUIDANCE=2, FULL=3
 *   - NIGHT_DATA (type 10): isNight boolean
 *
 * Optional sensors:
 *   - GPS_LOCATION (type 4): lat/lon/alt/bearing/speed
 *   - SPEED (type 3): speed in m/s * 1000 (millimeters/sec)
 */

import { CH, FRAME_FLAGS, SENSOR_TYPE } from '../constants.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

const SENSOR_MSG = {
  SENSOR_EVENT_INDICATION: 0x8001
} as const

export const DRIVING_STATUS = {
  UNRESTRICTED: 0,
  LIMIT_MESSAGE_LEN: 1, // shorten nav text
  LIMIT_GUIDANCE: 2, // no turn-by-turn voice
  FULL: 3 // full restriction (car in motion)
} as const

export class SensorChannel {
  constructor(private readonly _send: SendFn) {}

  /**
   * Send driving status to the phone.
   * Call with FULL when vehicle is moving, UNRESTRICTED when parked.
   */
  sendDrivingStatus(status: number): void {
    // SensorEventIndication with DrivingStatusData
    // field 1 (type 13): field 1 (status) = status value
    const inner = _encodeVarint32(0x08, status)
    const sensorEvent = Buffer.concat([
      _encodeVarint32(0x08, SENSOR_TYPE.DRIVING_STATUS), // field 1: sensor type
      _encodeLen(0x12, inner) // field 2: sensor data
    ])
    this._sendSensor(sensorEvent)
  }

  /**
   * Send night mode status to the phone.
   * @param isNight  true = night mode (dark theme), false = day mode
   */
  sendNightMode(isNight: boolean): void {
    // NightModeData: field 1 (isNight): bool
    const inner = _encodeVarint32(0x08, isNight ? 1 : 0)
    const sensorEvent = Buffer.concat([
      _encodeVarint32(0x08, SENSOR_TYPE.NIGHT_DATA),
      _encodeLen(0x12, inner)
    ])
    this._sendSensor(sensorEvent)
  }

  /**
   * Send GPS location to the phone.
   * All float fields use wire type 5 (32-bit fixed) as per proto definition.
   */
  sendGpsLocation(opts: {
    latitude: number // degrees
    longitude: number // degrees
    altitude?: number // meters
    bearing?: number // degrees (0 = north)
    speed?: number // m/s
    accuracy?: number // meters
  }): void {
    const parts: Buffer[] = []

    // LocationData fields (int32 millidegrees for lat/lon, int32 mm for alt, etc.)
    // lat/lon stored as int32 = degrees * 1e7
    parts.push(_encodeVarint32(0x08, Math.round(opts.latitude * 1e7)))
    parts.push(_encodeVarint32(0x10, Math.round(opts.longitude * 1e7)))
    if (opts.altitude !== undefined)
      parts.push(_encodeVarint32(0x18, Math.round(opts.altitude * 1000)))
    if (opts.bearing !== undefined)
      parts.push(_encodeVarint32(0x20, Math.round(opts.bearing * 1000)))
    if (opts.speed !== undefined) parts.push(_encodeVarint32(0x28, Math.round(opts.speed * 1000)))
    if (opts.accuracy !== undefined)
      parts.push(_encodeVarint32(0x30, Math.round(opts.accuracy * 1000)))

    const inner = Buffer.concat(parts)
    const sensorEvent = Buffer.concat([
      _encodeVarint32(0x08, SENSOR_TYPE.GPS_LOCATION),
      _encodeLen(0x12, inner)
    ])
    this._sendSensor(sensorEvent)
  }

  /**
   * Send an initial sensor burst on session start.
   * The phone requires at least DRIVING_STATUS + NIGHT_DATA before it will
   * show the main UI.
   */
  sendInitialSensors(opts: { isNight?: boolean; driving?: number } = {}): void {
    this.sendDrivingStatus(opts.driving ?? DRIVING_STATUS.UNRESTRICTED)
    this.sendNightMode(opts.isNight ?? false)
  }

  private _sendSensor(data: Buffer): void {
    this._send(CH.SENSOR, FRAME_FLAGS.ENC_SIGNAL, SENSOR_MSG.SENSOR_EVENT_INDICATION, data)
  }
}

// ── Protobuf encoding helpers (same as InputChannel, kept local) ──────────────

function _encodeVarint(value: number): Buffer {
  const bytes: number[] = []
  do {
    let b = value & 0x7f
    value >>>= 7
    if (value !== 0) b |= 0x80
    bytes.push(b)
  } while (value !== 0)
  return Buffer.from(bytes)
}

function _encodeVarint32(tag: number, value: number): Buffer {
  return Buffer.concat([_encodeVarint(tag), _encodeVarint(value)])
}

function _encodeLen(tag: number, data: Buffer): Buffer {
  return Buffer.concat([_encodeVarint(tag), _encodeVarint(data.length), data])
}
