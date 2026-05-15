import { CH, FRAME_FLAGS, SENSOR_TYPE } from '../../constants'
import { decodeFields, decodeVarintValue } from '../protoEnc'
import { DRIVING_STATUS, SensorChannel } from '../SensorChannel'

const SENSOR_EVENT_INDICATION = 0x8001

type Call = { channelId: number; flags: number; msgId: number; data: Buffer }

function freshSend(): { send: jest.Mock; calls: Call[] } {
  const calls: Call[] = []
  const send = jest.fn((channelId: number, flags: number, msgId: number, data: Buffer) => {
    calls.push({ channelId, flags, msgId, data })
  })
  return { send, calls }
}

function parseSensorEvent(data: Buffer): { sensorType: number; inner: Buffer } {
  const fields = Array.from(decodeFields(data))
  const sensorType = decodeVarintValue(fields[0].bytes)
  const inner = fields[1].bytes
  return { sensorType, inner }
}

describe('SensorChannel', () => {
  test('sendDrivingStatus emits SENSOR_EVENT_INDICATION on the SENSOR channel', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendDrivingStatus(DRIVING_STATUS.FULL)

    expect(calls).toHaveLength(1)
    expect(calls[0].channelId).toBe(CH.SENSOR)
    expect(calls[0].flags).toBe(FRAME_FLAGS.ENC_SIGNAL)
    expect(calls[0].msgId).toBe(SENSOR_EVENT_INDICATION)
  })

  test('sendDrivingStatus packs sensor type + status into the proto payload', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendDrivingStatus(DRIVING_STATUS.LIMIT_GUIDANCE)

    const { sensorType, inner } = parseSensorEvent(calls[0].data)
    expect(sensorType).toBe(SENSOR_TYPE.DRIVING_STATUS)
    expect(decodeVarintValue(Array.from(decodeFields(inner))[0].bytes)).toBe(
      DRIVING_STATUS.LIMIT_GUIDANCE
    )
  })

  test('sendNightMode encodes true/false as 1/0', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendNightMode(true)
    ch.sendNightMode(false)

    expect(calls).toHaveLength(2)
    expect(parseSensorEvent(calls[0].data).sensorType).toBe(SENSOR_TYPE.NIGHT_DATA)
    expect(
      decodeVarintValue(Array.from(decodeFields(parseSensorEvent(calls[0].data).inner))[0].bytes)
    ).toBe(1)
    expect(
      decodeVarintValue(Array.from(decodeFields(parseSensorEvent(calls[1].data).inner))[0].bytes)
    ).toBe(0)
  })

  test('sendGpsLocation packs lat/lon as int32 × 1e7', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendGpsLocation({ latitude: 52.5, longitude: 13.4 })

    const { sensorType, inner } = parseSensorEvent(calls[0].data)
    expect(sensorType).toBe(SENSOR_TYPE.GPS_LOCATION)

    const fields = Array.from(decodeFields(inner))
    expect(decodeVarintValue(fields[0].bytes)).toBe(525_000_000)
    expect(decodeVarintValue(fields[1].bytes)).toBe(134_000_000)
  })

  test('sendGpsLocation omits altitude/bearing/speed/accuracy when not given', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendGpsLocation({ latitude: 0, longitude: 0 })

    const { inner } = parseSensorEvent(calls[0].data)
    const fields = Array.from(decodeFields(inner))
    expect(fields).toHaveLength(2)
  })

  test('sendGpsLocation includes all optional fields when supplied', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendGpsLocation({
      latitude: 1,
      longitude: 2,
      altitude: 100,
      bearing: 90,
      speed: 25,
      accuracy: 5
    })

    const { inner } = parseSensorEvent(calls[0].data)
    const fields = Array.from(decodeFields(inner))
    expect(fields).toHaveLength(6)
    expect(decodeVarintValue(fields[2].bytes)).toBe(100_000)
    expect(decodeVarintValue(fields[3].bytes)).toBe(90_000)
    expect(decodeVarintValue(fields[4].bytes)).toBe(25_000)
    expect(decodeVarintValue(fields[5].bytes)).toBe(5_000)
  })

  test('sendInitialSensors sends DRIVING_STATUS=UNRESTRICTED + NIGHT_DATA=false by default', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendInitialSensors()

    expect(calls).toHaveLength(2)
    expect(parseSensorEvent(calls[0].data).sensorType).toBe(SENSOR_TYPE.DRIVING_STATUS)
    expect(parseSensorEvent(calls[1].data).sensorType).toBe(SENSOR_TYPE.NIGHT_DATA)
  })

  test('sendInitialSensors honours overrides', () => {
    const { send, calls } = freshSend()
    const ch = new SensorChannel(send)
    ch.sendInitialSensors({ isNight: true, driving: DRIVING_STATUS.FULL })

    const drivingInner = parseSensorEvent(calls[0].data).inner
    const nightInner = parseSensorEvent(calls[1].data).inner
    expect(decodeVarintValue(Array.from(decodeFields(drivingInner))[0].bytes)).toBe(
      DRIVING_STATUS.FULL
    )
    expect(decodeVarintValue(Array.from(decodeFields(nightInner))[0].bytes)).toBe(1)
  })

  test('DRIVING_STATUS enum-like has the expected values', () => {
    expect(DRIVING_STATUS).toEqual({
      UNRESTRICTED: 0,
      LIMIT_MESSAGE_LEN: 1,
      LIMIT_GUIDANCE: 2,
      FULL: 3
    })
  })
})
