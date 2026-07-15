import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CpManager } from '../CpManager'

type SessionLike = {
  getBtMac: () => string
  on: (event: string, listener: (arg: Record<string, unknown>) => void) => void
}

type Priv = {
  _onHelperEvent: (ev: Record<string, unknown>) => void
  _sessions: Set<SessionLike>
}

function makeManager(): Priv {
  const mgr = new CpManager({
    getConfig: () => ({}) as never,
    onSpawn: () => {},
    onHelperPresence: () => {}
  })
  return mgr as unknown as Priv
}

function sessionsFor(mgr: Priv, phoneId: string): SessionLike[] {
  const lower = phoneId.toLowerCase()
  return [...mgr._sessions].filter((s) => s.getBtMac().toLowerCase() === lower)
}

describe('CpManager session-at-identification', () => {
  beforeEach(() => {
    vi.spyOn(console, 'log').mockImplementation(() => {})
  })
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('births a session for a phoneId-tagged event that has no session yet', () => {
    const mgr = makeManager()
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: '0C:6A:C4:4E:F3:2A', title: 'X' })
    expect(sessionsFor(mgr, '0c:6a:c4:4e:f3:2a')).toHaveLength(1)
  })

  it('reuses the born session for further events of the same phone', () => {
    const mgr = makeManager()
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: '0c:6a', title: 'X' })
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: '0c:6a', elapsedMs: 1 })
    expect(sessionsFor(mgr, '0c:6a')).toHaveLength(1)
  })

  it('keeps different phones on different sessions', () => {
    const mgr = makeManager()
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: 'aa:aa', title: 'A' })
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: 'bb:bb', title: 'B' })
    expect(sessionsFor(mgr, 'aa:aa')).toHaveLength(1)
    expect(sessionsFor(mgr, 'bb:bb')).toHaveLength(1)
  })

  it('adopts a carkit usbUdid onto the session born from the same phoneId', () => {
    const mgr = makeManager()
    const phoneId = '0c:6a:c4:4e:f3:2a'
    const serial = '00008110-000A1B2C3D4E5F00'
    mgr._onHelperEvent({ type: 'nowplaying', phoneId, title: 'X' })
    const [session] = sessionsFor(mgr, phoneId)
    const adopted: Record<string, unknown>[] = []
    session?.on('device-presence', (p) => {
      if (p.kind === 'device') adopted.push(p)
    })
    mgr._onHelperEvent({ type: 'device', src: 'carkit', btMac: phoneId, usbUdid: serial })
    expect(sessionsFor(mgr, phoneId)).toHaveLength(1)
    expect(adopted.at(-1)?.usbUdid).toBe(serial)
    expect(adopted.at(-1)?.btMac).toBe(phoneId)
  })

  it('device-gone closes only the session matching that usbUdid', () => {
    const mgr = makeManager()
    const macA = 'aa:aa'
    const macB = 'bb:bb'
    const udidA = '00008110-000AAAAA'
    const udidB = '00008120-000BBBBB'
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: macA, title: 'A' })
    mgr._onHelperEvent({ type: 'nowplaying', phoneId: macB, title: 'B' })
    mgr._onHelperEvent({ type: 'device', src: 'carkit', btMac: macA, usbUdid: udidA })
    mgr._onHelperEvent({ type: 'device', src: 'carkit', btMac: macB, usbUdid: udidB })
    expect(sessionsFor(mgr, macA)).toHaveLength(1)
    expect(sessionsFor(mgr, macB)).toHaveLength(1)

    mgr._onHelperEvent({ type: 'device-gone', src: 'carkit', usbUdid: udidA })

    expect(sessionsFor(mgr, macA)).toHaveLength(0)
    expect(sessionsFor(mgr, macB)).toHaveLength(1)
  })
})
