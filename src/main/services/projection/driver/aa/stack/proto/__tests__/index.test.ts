import protobuf from 'protobufjs'
import { decode, encode } from '../index'

describe('proto/encode + decode helpers', () => {
  // Build a tiny dummy proto type inline so we don't depend on loaded protos.
  function tinyType(): protobuf.Type {
    const root = new protobuf.Root()
    const t = new protobuf.Type('Mini')
    t.add(new protobuf.Field('value', 1, 'int32'))
    root.add(t)
    return t
  }

  test('encode produces a Buffer', () => {
    const t = tinyType()
    const buf = encode(t, { value: 42 })
    expect(Buffer.isBuffer(buf)).toBe(true)
  })

  test('encode throws on verification failure', () => {
    const t = tinyType()
    expect(() => encode(t, { value: 'not-a-number' as unknown as number })).toThrow(
      /Proto encode error/
    )
  })

  test('encode → decode round-trip preserves fields', () => {
    const t = tinyType()
    const buf = encode(t, { value: 1234 })
    const obj = decode(t, buf)
    expect(obj.value).toBe(1234)
  })

  test('decode of an empty buffer yields an empty object', () => {
    const t = tinyType()
    expect(decode(t, Buffer.alloc(0))).toEqual({})
  })
})

describe('loadProtos — integration', () => {
  // resolveProtoRoot() picks `process.resourcesPath/aa/protos` first. Point it
  // at the in-tree protos so the loader can actually run in jest.
  // process.resourcesPath/aa/protos must resolve to the in-tree protos dir
  // (.../driver/aa/protos), so resourcesPath = .../driver
  const PROTO_PARENT = require('path').resolve(__dirname, '..', '..', '..', '..')

  beforeEach(() => {
    jest.resetModules()
    Object.defineProperty(process, 'resourcesPath', {
      value: PROTO_PARENT,
      configurable: true
    })
  })

  test('loads the real proto tree and exposes every declared type', async () => {
    const { loadProtos } = require('../index') as typeof import('../index')
    const types = await loadProtos()
    expect(types.ServiceDiscoveryResponse).toBeDefined()
    expect(types.ServiceDiscoveryRequest).toBeDefined()
    expect(types.ChannelOpenRequest).toBeDefined()
    expect(types.ChannelOpenResponse).toBeDefined()
    expect(types.PingRequest).toBeDefined()
    expect(types.PingResponse).toBeDefined()
    expect(types.AuthResponse).toBeDefined()
    expect(types.AuthCompleteIndication).toBe(types.AuthResponse)
    expect(types.Service).toBeDefined()
    expect(types.MediaSinkService).toBeDefined()
    expect(types.MediaSourceService).toBeDefined()
    expect(types.SensorSourceService).toBeDefined()
    expect(types.InputSourceService).toBeDefined()
    expect(types.BluetoothService).toBeDefined()
    expect(types.NavigationStatusService).toBeDefined()
    expect(types.MediaPlaybackStatusService).toBeDefined()
    expect(types.PhoneStatusService).toBeDefined()
    expect(types.AVChannelSetupRequest).toBeDefined()
    expect(types.AVChannelSetupResponse).toBeDefined()
    expect(types.AVChannelStartIndication).toBeDefined()
    expect(types.AVMediaAckIndication).toBeDefined()
    expect(types.BindingRequest).toBeDefined()
    expect(types.BindingResponse).toBeDefined()
  })

  test('second call returns the cached instance (no re-load)', async () => {
    const { loadProtos } = require('../index') as typeof import('../index')
    const a = await loadProtos()
    const b = await loadProtos()
    expect(a).toBe(b)
  })

  test('encode + decode round-trip a loaded ServiceDiscoveryRequest', async () => {
    const { loadProtos, encode, decode } = require('../index') as typeof import('../index')
    const types = await loadProtos()
    const buf = encode(types.ServiceDiscoveryRequest, { deviceName: 'phone' })
    const obj = decode(types.ServiceDiscoveryRequest, buf)
    expect(obj.deviceName).toBe('phone')
  })
})
