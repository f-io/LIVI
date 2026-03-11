import {
  MediaData,
  MediaType,
  Message,
  NavigationData,
  parseNaviInfoFromBuffer,
  SoftwareVersion
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

  test('parseNaviInfoFromBuffer parses json and strips trailing NUL', () => {
    const info = parseNaviInfoFromBuffer(Buffer.from('{"NaviStatus":1}\0\0', 'utf8'))
    expect(info).toEqual({ NaviStatus: 1 })
  })

  test('NavigationData stores rawUtf8 and parsed navi', () => {
    const buf = Buffer.from('{"NaviDestinationName":"Home"}\0', 'utf8')
    const msg = new NavigationData(fakeHeader() as any, 200 as any, buf)

    expect(msg.rawUtf8).toContain('NaviDestinationName')
    expect(msg.navi).toEqual({ NaviDestinationName: 'Home' })
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

  test('MediaData parses json media payload', () => {
    const json = JSON.stringify({ MediaSongName: 'Song' }) + '\0'
    const msg = new MediaData(fakeHeader() as any, MediaType.Data, Buffer.from(json, 'utf8'))

    expect(msg.payload).toEqual({
      type: MediaType.Data,
      media: { MediaSongName: 'Song' }
    })
  })
})
