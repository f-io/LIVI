import fs from 'fs'
import type { IPhoneDriver } from '../../driver/IPhoneDriver'
import type { NavigationData } from '../../messages'
import { NavStore } from '../NavStore'
import type { ProjectionSession } from '../SessionManager'

vi.mock('@shared/utils', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@shared/utils')>()
  return {
    ...actual,
    translateNavigation: vi.fn((_navi: unknown, locale: string) => ({
      SourceName: `app-${locale}`,
      DestinationName: 'dest',
      CurrentRoadName: 'road',
      AfterManeuverRoadName: 'after',
      ManeuverTypeText: 'maneuver',
      TimeRemainingToDestinationText: 'time',
      DistanceRemainingDisplayStringText: 'dist',
      RemainDistanceText: 'remain'
    }))
  }
})

function mkSession(nav: ProjectionSession['nav'] = null): ProjectionSession {
  return { nav } as unknown as ProjectionSession
}

function mkMsg(over: Record<string, unknown> = {}): NavigationData {
  return {
    metaType: 201,
    navi: { NaviStatus: 1 },
    rawUtf8: '',
    ...over
  } as unknown as NavigationData
}

describe('NavStore', () => {
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

  test('active session handling emits the raw message and persists the merged payload', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const driver = {} as IPhoneDriver
    const session = mkSession()
    const msg = mkMsg({ navi: { NaviStatus: 1, OrderType: 2 } })

    store.handle(driver, session, msg, true)

    expect(emit).toHaveBeenCalledWith({ type: 'navigation', payload: msg })
    expect(writeSpy).toHaveBeenCalledTimes(1)
    expect(writeSpy.mock.calls[0][0]).toBe('/tmp/navigationData.json')
    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload.display.appName).toBe('app-en')
    expect(written.payload.navi).toMatchObject({ NaviStatus: 1, OrderType: 2 })
    expect(session.nav?.display?.locale).toBe('en')
  })

  test('held session handling stores nav without emitting or persisting', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const session = mkSession()

    store.handle({} as IPhoneDriver, session, mkMsg(), false)

    expect(session.nav).not.toBeNull()
    expect(emit).not.toHaveBeenCalled()
    expect(writeSpy).not.toHaveBeenCalled()
  })

  test.each([
    ['de', 'de'],
    ['ua', 'ua'],
    ['fr', 'fr'],
    ['uk', 'ua'],
    ['uk-UA', 'ua'],
    ['en', 'en'],
    [undefined, 'en']
  ])('language %s maps to locale %s', (language, expected) => {
    const store = new NavStore({ emit: vi.fn(), getLanguage: () => language })
    const session = mkSession()

    store.handle({} as IPhoneDriver, session, mkMsg(), false)

    expect(session.nav?.display?.locale).toBe(expected)
    expect(session.nav?.display?.appName).toBe(`app-${expected}`)
  })

  test('sessionless messages accumulate in pending until a session adopts them', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const driver = {} as IPhoneDriver

    store.handle(driver, null, mkMsg({ navi: { A: 1 } }), false)
    store.handle(driver, null, mkMsg({ navi: { B: 2 } }), false)
    expect(emit).not.toHaveBeenCalled()

    const session = mkSession()
    store.handle(driver, session, mkMsg({ navi: { C: 3 } }), false)
    expect(session.nav?.navi).toMatchObject({ A: 1, B: 2, C: 3 })
  })

  test('adopting a session clears the pending snapshot for that driver', () => {
    const store = new NavStore({ emit: vi.fn(), getLanguage: () => 'en' })
    const driver = {} as IPhoneDriver

    store.handle(driver, null, mkMsg({ navi: { A: 1 } }), false)
    store.handle(driver, mkSession(), mkMsg(), false)

    const s2 = mkSession()
    store.handle(driver, null, mkMsg({ navi: { D: 4 } }), false)
    store.handle(driver, s2, mkMsg(), false)

    expect(s2.nav?.navi).toMatchObject({ D: 4 })
    expect(s2.nav?.navi).not.toHaveProperty('A')
  })

  test('an existing session nav is the merge base for the next message', () => {
    const store = new NavStore({ emit: vi.fn(), getLanguage: () => 'en' })
    const session = mkSession({
      metaType: 201,
      navi: { X: 1 },
      rawUtf8: '',
      error: false
    })

    store.handle({} as IPhoneDriver, session, mkMsg({ navi: { Y: 2 } }), false)

    expect(session.nav?.navi).toMatchObject({ X: 1, Y: 2 })
  })

  test('hydrate persists the session nav and emits a session-switch reset', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const nav = { metaType: 201, navi: { Z: 9 }, rawUtf8: '', error: false }

    store.hydrate(mkSession(nav))

    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload).toEqual(nav)
    expect(emit).toHaveBeenCalledWith({ type: 'navigation-reset', reason: 'session-switch' })
  })

  test('hydrate folds a parked payload into a fresh session and consumes it', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const driver = {} as IPhoneDriver
    store.handle(driver, null, mkMsg({ navi: { P: 1 } }))

    const session = mkSession()
    ;(session as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(session)
    expect(session.nav?.navi).toMatchObject({ P: 1 })

    const later = mkSession()
    ;(later as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(later)
    expect(later.nav).toBeNull()
  })

  test('hydrate keeps existing session nav over a parked snapshot', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    const driver = {} as IPhoneDriver
    store.handle(driver, null, mkMsg({ navi: { parked: 1 } }))

    const own = { metaType: 201, navi: { own: 1 }, rawUtf8: '', error: false }
    const session = mkSession(own)
    ;(session as unknown as { driver: IPhoneDriver }).driver = driver
    store.hydrate(session)
    expect(session.nav).toEqual(own)
  })

  test('hydrate falls back to the default payload when the session has no nav', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })

    store.hydrate(mkSession())

    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload.error).toBe(true)
    expect(written.payload.navi).toBeNull()
  })

  test('hydrate ignores write failures and still emits the reset', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    writeSpy.mockImplementationOnce(() => {
      throw new Error('disk full')
    })

    store.hydrate(mkSession())

    expect(warnSpy).toHaveBeenCalled()
    expect(emit).toHaveBeenCalledWith({ type: 'navigation-reset', reason: 'session-switch' })
  })

  test('reset persists the default payload and emits the reason', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })

    store.reset('phone-gone')

    const written = JSON.parse(writeSpy.mock.calls[0][1] as string)
    expect(written.payload.error).toBe(true)
    expect(emit).toHaveBeenCalledWith({ type: 'navigation-reset', reason: 'phone-gone' })
  })

  test('reset ignores write failures and still emits the reset', () => {
    const emit = vi.fn()
    const store = new NavStore({ emit, getLanguage: () => 'en' })
    writeSpy.mockImplementationOnce(() => {
      throw new Error('disk full')
    })

    store.reset('phone-gone')

    expect(warnSpy).toHaveBeenCalled()
    expect(emit).toHaveBeenCalledWith({ type: 'navigation-reset', reason: 'phone-gone' })
  })
})
