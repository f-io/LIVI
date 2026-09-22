import { PhoneType } from '@shared/types'
import fs from 'fs'
import type { IPhoneDriver } from '../../driver/IPhoneDriver'
import { type MediaData, MediaType } from '../../messages'
import { MediaStore, type MediaStoreDeps } from '../MediaStore'
import type { ProjectionSession } from '../SessionManager'

function mkSession(media: ProjectionSession['media'] = null): ProjectionSession {
  return { media } as unknown as ProjectionSession
}

function mkMsg(payload: unknown): MediaData {
  return { payload } as unknown as MediaData
}

function mkStore(over: Partial<MediaStoreDeps> = {}): {
  store: MediaStore
  emit: ReturnType<typeof vi.fn>
} {
  const emit = vi.fn()
  const store = new MediaStore({
    emit,
    getPlaybackInferred: () => 2,
    getLastPhoneType: () => PhoneType.CarPlay,
    ...over
  })
  return { store, emit }
}

describe('MediaStore', () => {
  let writeSpy: ReturnType<typeof vi.spyOn>
  let warnSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    writeSpy = vi.spyOn(fs, 'writeFileSync').mockImplementation(() => undefined)
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  })

  afterEach(() => {
    writeSpy.mockRestore()
    warnSpy.mockRestore()
  })

  test('reports deduped playback state changes for the active session', () => {
    const onPlaybackStatus = vi.fn()
    const { store } = mkStore({ onPlaybackStatus })
    const session = mkSession()

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 1 } }),
      true
    )
    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 1 } }),
      true
    )
    expect(onPlaybackStatus).toHaveBeenCalledTimes(1)
    expect(onPlaybackStatus).toHaveBeenCalledWith('playing')

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 2 } }),
      true
    )
    expect(onPlaybackStatus).toHaveBeenLastCalledWith('paused')
    expect(onPlaybackStatus).toHaveBeenCalledTimes(2)

    // The AA bridge encodes paused as 0.
    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 1 } }),
      true
    )
    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 0 } }),
      true
    )
    expect(onPlaybackStatus).toHaveBeenLastCalledWith('paused')
    expect(onPlaybackStatus).toHaveBeenCalledTimes(4)
  })

  test('ignores messages without a payload', () => {
    const { store, emit } = mkStore()
    const session = mkSession()

    store.handle({} as IPhoneDriver, session, mkMsg(undefined), true)

    expect(session.media).toBeNull()
    expect(emit).not.toHaveBeenCalled()
    expect(writeSpy).not.toHaveBeenCalled()
  })

  test('active session data merges over the default payload, persists and emits', () => {
    const { store, emit } = mkStore()
    const session = mkSession()

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaSongName: 'Song' } }),
      true
    )

    expect(session.media?.media).toMatchObject({ MediaSongName: 'Song', MediaAlbumName: '-' })
    expect(writeSpy).toHaveBeenCalledTimes(1)
    expect(writeSpy.mock.calls[0][0]).toBe('/tmp/mediaData.json')
    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload.media.MediaSongName).toBe('Song')
    expect(emit).toHaveBeenCalledWith({
      type: 'media',
      payload: { payload: { type: MediaType.Data, media: session.media?.media } }
    })
  })

  test('held session data is stored without persisting or emitting', () => {
    const { store, emit } = mkStore()
    const session = mkSession()

    store.handle({} as IPhoneDriver, session, mkMsg({ type: MediaType.Data, media: {} }), false)

    expect(session.media).not.toBeNull()
    expect(emit).not.toHaveBeenCalled()
    expect(writeSpy).not.toHaveBeenCalled()
  })

  test('android auto data without a play status gets the inferred one', () => {
    const { store } = mkStore({
      getLastPhoneType: () => PhoneType.AndroidAuto,
      getPlaybackInferred: () => 2
    })
    const session = mkSession({ type: MediaType.Data, media: {} })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaSongName: 'x' } }),
      false
    )

    expect(session.media?.media?.MediaPlayStatus).toBe(2)
  })

  test('android auto data keeps an explicit play status', () => {
    const { store } = mkStore({ getLastPhoneType: () => PhoneType.AndroidAuto })
    const session = mkSession({ type: MediaType.Data, media: {} })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.Data, media: { MediaPlayStatus: 1 } }),
      false
    )

    expect(session.media?.media?.MediaPlayStatus).toBe(1)
  })

  test('data updates carry the previous album art forward', () => {
    const { store } = mkStore()
    const session = mkSession({ type: MediaType.AlbumCoverAlt, base64Image: 'IMG' })

    store.handle({} as IPhoneDriver, session, mkMsg({ type: MediaType.Data, media: {} }), false)

    expect(session.media?.base64Image).toBe('IMG')
  })

  test('album art updates keep the previous media block', () => {
    const { store } = mkStore()
    const session = mkSession({ type: MediaType.Data, media: { MediaSongName: 'keep' } })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'NEW' }),
      false
    )

    expect(session.media?.base64Image).toBe('NEW')
    expect(session.media?.media).toEqual({ MediaSongName: 'keep' })
  })

  test('album art on a fresh session has no media block to carry', () => {
    const { store } = mkStore()
    const session = mkSession({ type: MediaType.AlbumCoverAlt })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'NEW' }),
      false
    )

    expect(session.media?.base64Image).toBe('NEW')
    expect(session.media?.media).toBeUndefined()
  })

  test('other payload types keep both media and image from the previous state', () => {
    const { store } = mkStore()
    const session = mkSession({
      type: MediaType.Data,
      media: { MediaSongName: 'keep' },
      base64Image: 'IMG'
    })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.ControlAutoplayTrigger }),
      false
    )

    expect(session.media?.type).toBe(MediaType.ControlAutoplayTrigger)
    expect(session.media?.media).toEqual({ MediaSongName: 'keep' })
    expect(session.media?.base64Image).toBe('IMG')
  })

  test('a data payload without media falls through to the passthrough branch', () => {
    const { store } = mkStore()
    const session = mkSession({ type: MediaType.Data, media: { MediaSongName: 'keep' } })

    store.handle({} as IPhoneDriver, session, mkMsg({ type: MediaType.Data }), false)

    expect(session.media?.media).toEqual({ MediaSongName: 'keep' })
  })

  test('an empty base64Image falls through to the passthrough branch', () => {
    const { store } = mkStore()
    const session = mkSession({ type: MediaType.Data, base64Image: 'IMG' })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: '' }),
      false
    )

    expect(session.media?.base64Image).toBe('IMG')
  })

  test('sessionless payloads accumulate in pending until a session adopts them', () => {
    const { store, emit } = mkStore()
    const driver = {} as IPhoneDriver

    store.handle(driver, null, mkMsg({ type: MediaType.Data, media: { A: 1 } }), false)
    store.handle(driver, null, mkMsg({ type: MediaType.Data, media: { B: 2 } }), false)
    expect(emit).not.toHaveBeenCalled()

    const session = mkSession()
    store.handle(driver, session, mkMsg({ type: MediaType.Data, media: { C: 3 } }), false)
    expect(session.media?.media).toMatchObject({ A: 1, B: 2, C: 3 })

    const s2 = mkSession()
    store.handle(driver, s2, mkMsg({ type: MediaType.Data, media: { D: 4 } }), false)
    expect(s2.media?.media).not.toHaveProperty('A')
    expect(s2.media?.media).toMatchObject({ D: 4 })
  })

  test('repeated emits only include the image when it changed', () => {
    const { store, emit } = mkStore()
    const session = mkSession()
    const driver = {} as IPhoneDriver

    store.handle(driver, session, mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'A' }), true)
    expect(emit.mock.calls[0][0].payload.payload.base64Image).toBe('A')

    store.handle(driver, session, mkMsg({ type: MediaType.Data, media: { x: 1 } }), true)
    expect(emit.mock.calls[1][0].payload.payload.base64Image).toBeUndefined()

    store.handle(driver, session, mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'B' }), true)
    expect(emit.mock.calls[2][0].payload.payload.base64Image).toBe('B')
  })

  test('patchAaPlayStatus ignores a missing session', () => {
    const { store, emit } = mkStore()

    store.patchAaPlayStatus(null, 1)

    expect(emit).not.toHaveBeenCalled()
    expect(writeSpy).not.toHaveBeenCalled()
  })

  test('patchAaPlayStatus patches the session media, persists and emits', () => {
    const { store, emit } = mkStore()
    const session = mkSession({ type: MediaType.AlbumCoverAlt, media: { MediaSongName: 's' } })

    store.patchAaPlayStatus(session, 2)

    expect(session.media?.type).toBe(MediaType.Data)
    expect(session.media?.media).toMatchObject({ MediaSongName: 's', MediaPlayStatus: 2 })
    expect(writeSpy).toHaveBeenCalledTimes(1)
    expect(emit).toHaveBeenCalledTimes(1)
  })

  test('patchAaPlayStatus starts from the default payload and swallows write errors', () => {
    const { store, emit } = mkStore()
    const session = mkSession()
    writeSpy.mockImplementationOnce(() => {
      throw new Error('disk full')
    })

    store.patchAaPlayStatus(session, 1)

    expect(session.media?.media?.MediaPlayStatus).toBe(1)
    expect(warnSpy).toHaveBeenCalled()
    expect(emit).not.toHaveBeenCalled()
  })

  test('hydrate persists the session media and emits a session-switch reset', () => {
    const { store, emit } = mkStore()
    const session = mkSession({ type: MediaType.Data, media: { x: 1 }, base64Image: 'IMG' })

    store.hydrate(session)

    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload.media).toEqual({ x: 1 })
    expect(emit).toHaveBeenCalledWith({ type: 'media-reset', reason: 'session-switch' })

    store.handle(
      {} as IPhoneDriver,
      session,
      mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'IMG' }),
      true
    )
    expect(emit.mock.calls[1][0].payload.payload.base64Image).toBeUndefined()
  })

  test('hydrate folds a parked payload into a fresh session and consumes it', () => {
    const { store } = mkStore()
    const driver = {} as IPhoneDriver
    store.handle(driver, null, mkMsg({ type: MediaType.Data, media: { A: 1 } }), false)

    const session = mkSession()
    ;(session as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(session)
    expect(session.media?.media).toMatchObject({ A: 1 })

    const later = mkSession()
    ;(later as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(later)
    expect(later.media).toBeNull()
  })

  test('hydrate keeps existing session media over a parked snapshot', () => {
    const { store } = mkStore()
    const driver = {} as IPhoneDriver
    store.handle(driver, null, mkMsg({ type: MediaType.Data, media: { parked: 1 } }), false)

    const session = mkSession({ type: MediaType.Data, media: { own: 1 } })
    ;(session as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(session)
    expect(session.media?.media).toEqual({ own: 1 })
  })

  test('hydrate falls back to the default payload and swallows write errors', () => {
    const { store, emit } = mkStore()
    writeSpy.mockImplementationOnce(() => {
      throw new Error('disk full')
    })

    store.hydrate(mkSession())

    expect(warnSpy).toHaveBeenCalled()
    expect(emit).toHaveBeenCalledWith({ type: 'media-reset', reason: 'session-switch' })
  })

  test('reset persists the default payload, clears the image cache and emits the reason', () => {
    const { store, emit } = mkStore()
    const session = mkSession()
    const driver = {} as IPhoneDriver
    store.handle(driver, session, mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'A' }), true)

    store.reset('phone-gone')

    const written = JSON.parse(writeSpy.mock.calls[1][1] as string)
    expect(written.payload.error).toBe(true)
    expect(emit).toHaveBeenCalledWith({ type: 'media-reset', reason: 'phone-gone' })

    store.handle(driver, session, mkMsg({ type: MediaType.AlbumCoverAlt, base64Image: 'A' }), true)
    expect(emit.mock.calls[2][0].payload.payload.base64Image).toBe('A')
  })

  test('reset swallows write errors and still emits', () => {
    const { store, emit } = mkStore()
    writeSpy.mockImplementationOnce(() => {
      throw new Error('disk full')
    })

    store.reset('phone-gone')

    expect(warnSpy).toHaveBeenCalled()
    expect(emit).toHaveBeenCalledWith({ type: 'media-reset', reason: 'phone-gone' })
  })
})
