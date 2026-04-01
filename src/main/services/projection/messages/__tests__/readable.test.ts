import {
  AudioData,
  BluetoothAddress,
  BluetoothDeviceName,
  BluetoothPIN,
  BluetoothPairedList,
  BoxInfo,
  BoxPhase,
  BoxUpdateProgress,
  BoxUpdateState,
  BoxUpdateStatus,
  boxPhaseToString,
  boxUpdateStatusToString,
  Command,
  DongleReady,
  GnssData,
  HiCarLink,
  ManufacturerInfo,
  MediaData,
  MediaType,
  Message,
  MetaData,
  NavigationData,
  NavigationMetaType,
  Opened,
  parseNaviInfoFromBuffer,
  Phase,
  PhoneType,
  Plugged,
  SoftwareVersion,
  Unplugged,
  VendorSessionInfo,
  VideoData,
  WifiDeviceName
} from '@main/services/projection/messages/readable'

function fakeHeader(): Message['header'] {
  return { length: 0, type: 0 } as any
}

describe('readable messages', () => {
  test('SoftwareVersion keeps normalized yyyy.mm.dd.hhmm form', () => {
    const msg = new SoftwareVersion(
      fakeHeader() as any,
      Buffer.from('2025.03.19.1126-beta\0', 'ascii')
    )
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('SoftwareVersion keeps plain version when no beta suffix exists', () => {
    const msg = new SoftwareVersion(fakeHeader() as any, Buffer.from('2025.03.19.1126\0', 'ascii'))
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('SoftwareVersion trims trailing whitespace and NUL bytes', () => {
    const msg = new SoftwareVersion(
      fakeHeader() as any,
      Buffer.from('2025.03.19.1126   \0\0', 'ascii')
    )
    expect(msg.version).toBe('2025.03.19.1126')
  })

  test('parseNaviInfoFromBuffer parses json and strips trailing NUL', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{"NaviStatus":1}\0\0', 'utf8'))
    expect(info).toEqual({ NaviStatus: 1 })
  })

  test('parseNaviInfoFromBuffer returns null for invalid json', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{not-json}\0', 'utf8'))
    expect(info).toBeNull()
  })

  test('parseNaviInfoFromBuffer returns null for empty payload', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('\0\0', 'utf8'))
    expect(info).toBeNull()
  })

  test('NavigationData stores rawUtf8 and parsed navi', () => {
    const buf = Buffer.from('{"NaviDestinationName":"Home"}\0', 'utf8')
    const msg = new NavigationData(fakeHeader() as any, 200 as any, buf)

    expect(msg.rawUtf8).toContain('NaviDestinationName')
    expect(msg.navi).toEqual({ NaviDestinationName: 'Home' })
  })

  test('NavigationData handles invalid json and preserves rawUtf8', () => {
    const buf = Buffer.from('{oops}\0', 'utf8')
    const msg = new NavigationData(fakeHeader() as any, 200 as any, buf)

    expect(msg.rawUtf8).toContain('{oops}')
    expect(msg.navi).toBeNull()
  })

  test('NavigationData dashboard image stores base64 image and empty rawUtf8', () => {
    const raw = Buffer.from([1, 2, 3, 4])
    const msg = new NavigationData(fakeHeader() as any, NavigationMetaType.DashboardImage, raw)

    expect(msg.rawUtf8).toBe('')
    expect(msg.navi).toEqual({
      NaviImageBase64: raw.toString('base64')
    })
  })

  test('MediaData handles album cover ascii-base64 payload', () => {
    const b64 = Buffer.from('abcd', 'utf8').toString('base64')
    const msg = new MediaData(
      fakeHeader() as any,
      MediaType.AlbumCoverAlt,
      Buffer.from(b64 + '\0', 'ascii')
    )

    expect(msg.payload).toEqual({ type: MediaType.AlbumCoverAlt, base64Image: b64 })
  })

  test('MediaData handles standard album cover ascii-base64 payload', () => {
    const b64 = Buffer.from('cover', 'utf8').toString('base64')
    const msg = new MediaData(fakeHeader() as any, MediaType.AlbumCover, Buffer.from(b64 + '\0'))

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: b64
    })
  })

  test('MediaData encodes raw binary album cover as base64', () => {
    const raw = Buffer.from([0xde, 0xad, 0xbe, 0xef])
    const msg = new MediaData(fakeHeader() as any, MediaType.AlbumCoverAlt, raw)

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: raw.toString('base64')
    })
  })

  test('MediaData parses json media payload', () => {
    const json = JSON.stringify({ MediaSongName: 'Song' }) + '\0'
    const msg = new MediaData(fakeHeader() as any, MediaType.Data, Buffer.from(json, 'utf8'))

    expect(msg.payload).toEqual({
      type: MediaType.Data,
      media: { MediaSongName: 'Song' }
    })
  })

  test('MediaData does not expose payload for json media payload with trailing double NUL bytes', () => {
    const json = JSON.stringify({ MediaSongName: 'Song', MediaArtistName: 'Artist' }) + '\0\0'
    const msg = new MediaData(fakeHeader() as any, MediaType.Data, Buffer.from(json, 'utf8'))

    expect(msg.payload).toBeUndefined()
  })

  test('MediaData does not expose payload for invalid json data payload', () => {
    const msg = new MediaData(fakeHeader() as any, MediaType.Data, Buffer.from('{bad}\0', 'utf8'))

    expect(msg.payload).toBeUndefined()
  })

  test('MediaData handles autoplay trigger payload', () => {
    const msg = new MediaData(
      fakeHeader() as any,
      MediaType.ControlAutoplayTrigger,
      Buffer.alloc(0)
    )

    expect(msg.payload).toEqual({ type: MediaType.ControlAutoplayTrigger })
  })

  test('MediaData keeps unknown media type payload undefined', () => {
    const msg = new MediaData(fakeHeader() as any, 999 as any, Buffer.from('abc\0', 'utf8'))

    expect(msg.payload).toBeUndefined()
  })

  test('DongleReady and Unplugged construct from header', () => {
    const ready = new DongleReady(fakeHeader() as any)
    const unplugged = new Unplugged(fakeHeader() as any)

    expect(ready.header).toEqual(fakeHeader())
    expect(unplugged.header).toEqual(fakeHeader())
  })

  test('Command reads uint32 command value', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(123, 0)

    const msg = new Command(fakeHeader() as any, buf)

    expect(msg.value).toBe(123)
  })

  test('ManufacturerInfo reads two uint32 values', () => {
    const buf = Buffer.alloc(8)
    buf.writeUInt32LE(11, 0)
    buf.writeUInt32LE(22, 4)

    const msg = new ManufacturerInfo(fakeHeader() as any, buf)

    expect(msg.a).toBe(11)
    expect(msg.b).toBe(22)
  })

  test('GnssData strips trailing NUL bytes', () => {
    const msg = new GnssData(fakeHeader() as any, Buffer.from('$GPGGA\0\0', 'ascii'))
    expect(msg.text).toBe('$GPGGA')
  })

  test('Bluetooth/Wifi/HiCar messages keep ascii text', () => {
    expect(new BluetoothAddress(fakeHeader() as any, Buffer.from('AA:BB', 'ascii')).address).toBe(
      'AA:BB'
    )
    expect(new BluetoothPIN(fakeHeader() as any, Buffer.from('1234', 'ascii')).pin).toBe('1234')
    expect(
      new BluetoothDeviceName(fakeHeader() as any, Buffer.from('My Phone', 'ascii')).name
    ).toBe('My Phone')
    expect(new WifiDeviceName(fakeHeader() as any, Buffer.from('Car Wifi', 'ascii')).name).toBe(
      'Car Wifi'
    )
    expect(new HiCarLink(fakeHeader() as any, Buffer.from('link://abc', 'ascii')).link).toBe(
      'link://abc'
    )
  })

  test('BluetoothPairedList decodes utf8 and strips trailing NUL', () => {
    const msg = new BluetoothPairedList(fakeHeader() as any, Buffer.from('Gerät A\0', 'utf8'))
    expect(msg.data).toBe('Gerät A')
  })

  test('Plugged reads phone type only when payload is 4 bytes', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(PhoneType.CarPlay, 0)

    const msg = new Plugged(fakeHeader() as any, buf)

    expect(msg.phoneType).toBe(PhoneType.CarPlay)
    expect(msg.wifi).toBeUndefined()
  })

  test('Plugged reads phone type and wifi when payload is 8 bytes', () => {
    const buf = Buffer.alloc(8)
    buf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    buf.writeUInt32LE(1, 4)

    const msg = new Plugged(fakeHeader() as any, buf)

    expect(msg.phoneType).toBe(PhoneType.AndroidAuto)
    expect(msg.wifi).toBe(1)
  })

  test('AudioData without extra payload only reads header fields', () => {
    const buf = Buffer.alloc(12)
    buf.writeUInt32LE(5, 0)
    buf.writeFloatLE(0.75, 4)
    buf.writeUInt32LE(9, 8)

    const msg = new AudioData(fakeHeader() as any, buf)

    expect(msg.decodeType).toBe(5)
    expect(msg.volume).toBeCloseTo(0.75)
    expect(msg.audioType).toBe(9)
    expect(msg.command).toBeUndefined()
    expect(msg.volumeDuration).toBeUndefined()
    expect(msg.data).toBeUndefined()
  })

  test('AudioData reads 1-byte command payload', () => {
    const buf = Buffer.alloc(13)
    buf.writeUInt32LE(5, 0)
    buf.writeFloatLE(1.0, 4)
    buf.writeUInt32LE(2, 8)
    buf.writeUInt8(7, 12)

    const msg = new AudioData(fakeHeader() as any, buf)

    expect(msg.command).toBe(7)
    expect(msg.data).toBeUndefined()
  })

  test('AudioData reads 4-byte volumeDuration payload', () => {
    const buf = Buffer.alloc(16)
    buf.writeUInt32LE(5, 0)
    buf.writeFloatLE(1.0, 4)
    buf.writeUInt32LE(2, 8)
    buf.writeFloatLE(2.5, 12)

    const msg = new AudioData(fakeHeader() as any, buf)

    expect(msg.volumeDuration).toBeCloseTo(2.5)
    expect(msg.command).toBeUndefined()
  })

  test('AudioData reads pcm data payload into Int16Array', () => {
    const pcm = new Int16Array([100, -200, 300])
    const head = Buffer.alloc(12)
    head.writeUInt32LE(5, 0)
    head.writeFloatLE(1.0, 4)
    head.writeUInt32LE(2, 8)

    const msg = new AudioData(fakeHeader() as any, Buffer.concat([head, Buffer.from(pcm.buffer)]))

    expect(Array.from(msg.data ?? [])).toEqual([100, -200, 300])
  })

  test('VideoData reads dimensions, flags and binary payload', () => {
    const buf = Buffer.alloc(24)
    buf.writeUInt32LE(800, 0)
    buf.writeUInt32LE(480, 4)
    buf.writeUInt32LE(1, 8)
    buf.writeUInt32LE(4, 12)
    buf.writeUInt32LE(99, 16)
    buf.writeUInt32LE(0xaabbccdd, 20)

    const msg = new VideoData(fakeHeader() as any, buf)

    expect(msg.width).toBe(800)
    expect(msg.height).toBe(480)
    expect(msg.flags).toBe(1)
    expect(msg.length).toBe(4)
    expect(msg.unknown).toBe(99)
    expect(msg.data.length).toBe(4)
  })

  test('MetaData wraps navigation payloads', () => {
    const innerType = Buffer.alloc(4)
    innerType.writeUInt32LE(NavigationMetaType.DashboardInfo, 0)
    const body = Buffer.from('{"NaviStatus":1}\0', 'utf8')

    const msg = new MetaData(fakeHeader() as any, Buffer.concat([innerType, body]))

    expect(msg.innerType).toBe(NavigationMetaType.DashboardInfo)
    expect(msg.inner.kind).toBe('navigation')
    if (msg.inner.kind === 'navigation') {
      expect(msg.inner.message.navi).toEqual({ NaviStatus: 1 })
    }
  })

  test('MetaData wraps media payloads', () => {
    const innerType = Buffer.alloc(4)
    innerType.writeUInt32LE(MediaType.AlbumCoverAlt, 0)
    const body = Buffer.from(Buffer.from('cover').toString('base64') + '\0', 'ascii')

    const msg = new MetaData(fakeHeader() as any, Buffer.concat([innerType, body]))

    expect(msg.innerType).toBe(MediaType.AlbumCoverAlt)
    expect(msg.inner.kind).toBe('media')
    if (msg.inner.kind === 'media') {
      expect(msg.inner.message.payload).toEqual({
        type: MediaType.AlbumCoverAlt,
        base64Image: Buffer.from('cover').toString('base64')
      })
    }
  })

  test('MetaData keeps unknown payloads as unknown inner kind', () => {
    const infoSpy = jest.spyOn(console, 'info').mockImplementation(() => {})
    const innerType = Buffer.alloc(4)
    innerType.writeUInt32LE(999, 0)
    const body = Buffer.from('mystery\0', 'utf8')

    const msg = new MetaData(fakeHeader() as any, Buffer.concat([innerType, body]))

    expect(msg.innerType).toBe(999)
    expect(msg.inner).toEqual({
      kind: 'unknown',
      metaType: 999,
      raw: body
    })
    expect(infoSpy).toHaveBeenCalled()

    infoSpy.mockRestore()
  })

  test('Opened reads open payload fields', () => {
    const buf = Buffer.alloc(28)
    buf.writeUInt32LE(800, 0)
    buf.writeUInt32LE(480, 4)
    buf.writeUInt32LE(60, 8)
    buf.writeUInt32LE(5, 12)
    buf.writeUInt32LE(49152, 16)
    buf.writeUInt32LE(2, 20)
    buf.writeUInt32LE(3, 24)

    const msg = new Opened(fakeHeader() as any, buf)

    expect(msg.width).toBe(800)
    expect(msg.height).toBe(480)
    expect(msg.fps).toBe(60)
    expect(msg.format).toBe(5)
    expect(msg.packetMax).toBe(49152)
    expect(msg.iBox).toBe(2)
    expect(msg.phoneMode).toBe(3)
  })

  test('BoxInfo parses json settings', () => {
    const msg = new BoxInfo(
      fakeHeader() as any,
      Buffer.from(
        JSON.stringify({
          uuid: 'u1',
          MFD: 'm1',
          productType: 'A15W',
          DevList: [{ id: '1', name: 'Phone' }]
        }),
        'utf8'
      )
    )

    expect(msg.settings).toEqual({
      uuid: 'u1',
      MFD: 'm1',
      productType: 'A15W',
      DevList: [{ id: '1', name: 'Phone' }]
    })
  })

  test('VendorSessionInfo keeps raw buffer', () => {
    const raw = Buffer.from('secret')
    const msg = new VendorSessionInfo(fakeHeader() as any, raw)

    expect(msg.raw).toBe(raw)
  })

  test('Phase reads uint32 value', () => {
    const buf = Buffer.alloc(4)
    buf.writeUInt32LE(BoxPhase.EVT_BOX_READY, 0)

    const msg = new Phase(fakeHeader() as any, buf)

    expect(msg.value).toBe(BoxPhase.EVT_BOX_READY)
  })

  test('boxPhaseToString returns enum name and fallback for unknown values', () => {
    expect(boxPhaseToString(BoxPhase.EVT_BOX_READY)).toBe('EVT_BOX_READY')
    expect(boxPhaseToString(9999)).toBe('UNKNOWN_PHASE_9999')
  })

  test('boxUpdateStatusToString maps known statuses and unknown fallback', () => {
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateStart)).toBe('EVT_BOX_UPDATE')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateSuccess)).toBe('EVT_BOX_UPDATE_SUCCESS')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxUpdateFailed)).toBe('EVT_BOX_UPDATE_FAILED')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateStart)).toBe('EVT_BOX_OTA_UPDATE')
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateSuccess)).toBe(
      'EVT_BOX_OTA_UPDATE_SUCCESS'
    )
    expect(boxUpdateStatusToString(BoxUpdateStatus.BoxOtaUpdateFailed)).toBe(
      'EVT_BOX_OTA_UPDATE_FAILED'
    )
    expect(boxUpdateStatusToString(999)).toBe('EVT_BOX_UPDATE_UNKNOWN(999)')
  })

  test('BoxUpdateProgress reads signed progress int32', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(-12, 0)

    const msg = new BoxUpdateProgress(fakeHeader() as any, buf)

    expect(msg.progress).toBe(-12)
  })

  test('BoxUpdateState maps success terminal state', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(BoxUpdateStatus.BoxUpdateSuccess, 0)

    const msg = new BoxUpdateState(fakeHeader() as any, buf)

    expect(msg.status).toBe(BoxUpdateStatus.BoxUpdateSuccess)
    expect(msg.statusText).toBe('EVT_BOX_UPDATE_SUCCESS')
    expect(msg.isOta).toBe(false)
    expect(msg.isTerminal).toBe(true)
    expect(msg.ok).toBe(true)
  })

  test('BoxUpdateState maps failed ota terminal state', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(BoxUpdateStatus.BoxOtaUpdateFailed, 0)

    const msg = new BoxUpdateState(fakeHeader() as any, buf)

    expect(msg.status).toBe(BoxUpdateStatus.BoxOtaUpdateFailed)
    expect(msg.statusText).toBe('EVT_BOX_OTA_UPDATE_FAILED')
    expect(msg.isOta).toBe(true)
    expect(msg.isTerminal).toBe(true)
    expect(msg.ok).toBe(false)
  })

  test('BoxUpdateState maps non-terminal start state', () => {
    const buf = Buffer.alloc(4)
    buf.writeInt32LE(BoxUpdateStatus.BoxUpdateStart, 0)

    const msg = new BoxUpdateState(fakeHeader() as any, buf)

    expect(msg.isTerminal).toBe(false)
    expect(msg.ok).toBeUndefined()
  })

  test('AudioData reads non-command non-duration extra payload as Int16Array', () => {
    const head = Buffer.alloc(12)
    head.writeUInt32LE(5, 0)
    head.writeFloatLE(1.0, 4)
    head.writeUInt32LE(2, 8)

    const pcmBytes = Buffer.from([0x34, 0x12]) // ein Int16-Wert = 0x1234
    const msg = new AudioData(fakeHeader() as any, Buffer.concat([head, pcmBytes]))

    expect(msg.command).toBeUndefined()
    expect(msg.volumeDuration).toBeUndefined()
    expect(msg.data).toBeInstanceOf(Int16Array)
    expect(Array.from(msg.data ?? [])).toEqual([0x1234])
  })

  test('parseNaviInfoFromBuffer cuts off content at first NUL byte', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{"NaviStatus":1}\0TRAILING', 'utf8'))
    expect(info).toEqual({ NaviStatus: 1 })
  })

  test('parseNaviInfoFromBuffer returns null for valid non-object json', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('123', 'utf8'))
    expect(info).toBeNull()
  })

  test('NavigationData cuts off text at first NUL byte', () => {
    const buf = Buffer.from('{"NaviStatus":1}\0TRAILING', 'utf8')
    const msg = new NavigationData(fakeHeader() as any, NavigationMetaType.DashboardInfo, buf)

    expect(msg.rawUtf8).toBe('{"NaviStatus":1}')
    expect(msg.navi).toEqual({ NaviStatus: 1 })
  })

  test('MediaData treats empty album cover ascii payload as non-base64 and encodes raw bytes', () => {
    const raw = Buffer.from('\0\0', 'ascii')
    const msg = new MediaData(fakeHeader() as any, MediaType.AlbumCoverAlt, raw)

    expect(msg.payload).toEqual({
      type: MediaType.AlbumCoverAlt,
      base64Image: raw.toString('base64')
    })
  })

  test('AudioData reads non-command non-duration extra payload as Int16Array', () => {
    const head = Buffer.alloc(12)
    head.writeUInt32LE(5, 0)
    head.writeFloatLE(1.0, 4)
    head.writeUInt32LE(2, 8)

    const pcmBytes = Buffer.from([0x34, 0x12])
    const msg = new AudioData(fakeHeader() as any, Buffer.concat([head, pcmBytes]))

    expect(msg.command).toBeUndefined()
    expect(msg.volumeDuration).toBeUndefined()
    expect(msg.data).toBeInstanceOf(Int16Array)
    expect(Array.from(msg.data ?? [])).toEqual([0x1234])
  })

  test('NavigationData keeps rawUtf8 unchanged when payload has no NUL byte', () => {
    const buf = Buffer.from('{"NaviDestinationName":"Home"}', 'utf8')
    const msg = new NavigationData(fakeHeader() as any, NavigationMetaType.DashboardInfo, buf)

    expect(msg.rawUtf8).toBe('{"NaviDestinationName":"Home"}')
    expect(msg.navi).toEqual({ NaviDestinationName: 'Home' })
  })
})
