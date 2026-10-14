import type { FromCore, State, ToCore } from '../contract'
import { CoreSession } from '../session'

const state = (huVolume: number) =>
  ({
    front: { main: 'livi', cluster: 'livi', aux: 'livi' },
    config: { huVolume }
  }) as unknown as State

const welcome = (rev: number, s: State): FromCore => ({
  type: 'welcome',
  protocol: 1,
  version: '0',
  rev,
  state: s
})

function setup(connected = true) {
  const sent: ToCore[] = []
  const session = new CoreSession((msg) => {
    sent.push(msg)
    return connected
  })
  return { session, sent }
}

describe('CoreSession', () => {
  test('mirrors the welcome and the patches after it', () => {
    const { session } = setup()
    const seen: Array<[State, State | null]> = []
    session.subscribe((s, prev) => seen.push([s, prev]))
    const first = state(0.5)
    session.receive(welcome(4, first))
    session.receive({
      type: 'patch',
      rev: 5,
      ops: [{ op: 'set', path: ['config', 'huVolume'], value: 0.9 }]
    })
    expect(seen[0]).toEqual([first, null])
    expect(seen[1][0].config.huVolume).toBe(0.9)
    expect(seen[1][1]).toBe(first)
    expect(session.state?.config.huVolume).toBe(0.9)
  })

  test('asks for the whole state when a patch does not fit', () => {
    const { session, sent } = setup()
    session.receive({ type: 'patch', rev: 1, ops: [] })
    session.receive(welcome(1, state(0.5)))
    session.receive({ type: 'patch', rev: 3, ops: [] })
    session.receive({ type: 'patch', rev: 2, ops: [{ op: 'remove', path: ['nope'] }] })
    expect(sent).toEqual([{ type: 'resync' }, { type: 'resync' }, { type: 'resync' }])
    expect(session.state?.config.huVolume).toBe(0.5)
  })

  test('an action waits for its reply', async () => {
    const { session, sent } = setup()
    const ok = session.act({ kind: 'quit' })
    const bad = session.act({ kind: 'nextDevice' })
    expect(sent).toEqual([
      { type: 'action', id: 1, action: { kind: 'quit' } },
      { type: 'action', id: 2, action: { kind: 'nextDevice' } }
    ])
    session.receive({ type: 'reply', id: 99 })
    session.receive({ type: 'reply', id: 2, error: 'not yet' })
    session.receive({ type: 'reply', id: 1 })
    await expect(ok).resolves.toBeUndefined()
    await expect(bad).rejects.toThrow('not yet')
  })

  test('an action without a connection fails at once', async () => {
    const { session } = setup(false)
    await expect(session.act({ kind: 'quit' })).rejects.toThrow('core is not connected')
  })

  test('a lost connection fails what still waits', async () => {
    const { session } = setup()
    const pending = session.act({ kind: 'restart' })
    session.disconnected()
    await expect(pending).rejects.toThrow('core went away')
  })

  test('the path is sent again after a welcome', () => {
    const { session, sent } = setup()
    session.receive(welcome(0, state(0.5)))
    session.path('/media')
    session.receive(welcome(0, state(0.5)))
    expect(sent).toEqual([
      { type: 'path', path: '/media' },
      { type: 'path', path: '/media' }
    ])
  })

  test('what a screen shows is sent again after a welcome', () => {
    const { session, sent } = setup()
    session.show('main', 'livi')
    session.show('main', 'projection')
    session.receive(welcome(0, state(0.5)))
    const show = (id: number, front: string) => ({
      type: 'action',
      id,
      action: { kind: 'show', screen: 'main', front }
    })
    expect(sent).toEqual([show(1, 'livi'), show(2, 'projection'), show(3, 'projection')])
  })

  test('drawing the spectrum is told again after a welcome and its frames reach the listeners', () => {
    const { session, sent } = setup()
    const frames: number[][] = []
    const stop = session.onSpectrum((bands) => frames.push(bands))
    session.spectrum(true)
    session.receive(welcome(0, state(0.5)))
    session.receive({ type: 'spectrum', bands: [0.25, 1] })
    session.spectrum(false)
    session.receive(welcome(0, state(0.5)))
    stop()
    session.receive({ type: 'spectrum', bands: [0] })
    expect(sent).toEqual([
      { type: 'spectrum', on: true },
      { type: 'spectrum', on: true },
      { type: 'spectrum', on: false }
    ])
    expect(frames).toEqual([[0.25, 1]])
  })

  test('showing the link speed is told again after a welcome until it closes', () => {
    const { session, sent } = setup()
    session.linkSpeed(true)
    session.receive(welcome(0, state(0.5)))
    session.linkSpeed(false)
    session.receive(welcome(0, state(0.5)))
    expect(sent).toEqual([
      { type: 'linkSpeed', on: true },
      { type: 'linkSpeed', on: true },
      { type: 'linkSpeed', on: false }
    ])
  })

  test('whether a camera is there is told again after a welcome', () => {
    const { session, sent } = setup()
    session.receive(welcome(0, state(0.5)))
    session.camera(true)
    session.camera(false)
    session.receive(welcome(0, state(0.5)))
    expect(sent).toEqual([
      { type: 'camera', found: true },
      { type: 'camera', found: false },
      { type: 'camera', found: false }
    ])
  })

  test('a page core asks for reaches the listeners until they stop', () => {
    const { session } = setup()
    const pages: string[] = []
    const stop = session.onOpenPage((path) => pages.push(path))
    session.receive({ type: 'openPage', path: '/media' })
    stop()
    session.receive({ type: 'openPage', path: '/settings' })
    expect(pages).toEqual(['/media'])
  })

  test('input goes out as it is and is not kept', () => {
    const { session, sent } = setup()
    session.input({ kind: 'key', code: 'KeyN', down: true })
    session.receive(welcome(0, state(0.5)))
    expect(sent).toEqual([{ type: 'input', input: { kind: 'key', code: 'KeyN', down: true } }])
  })

  test('showing without a connection waits for the welcome', async () => {
    const { session, sent } = setup(false)
    session.show('cluster', 'cluster')
    await Promise.resolve()
    expect(sent).toHaveLength(1)
  })

  test('a refusal is logged and an unsubscribed listener hears nothing', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const { session } = setup()
    const listener = vi.fn()
    const off = session.subscribe(listener)
    off()
    session.receive({ type: 'refused', reason: 'old' })
    session.receive(welcome(0, state(0.5)))
    expect(error).toHaveBeenCalledWith('[core] refused: old')
    expect(listener).not.toHaveBeenCalled()
    error.mockRestore()
  })
})
