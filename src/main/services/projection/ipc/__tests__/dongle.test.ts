type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerDongleIpc } from '../dongle'

function makeFw(over: Record<string, unknown> = {}) {
  return {
    checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: false })),
    downloadFirmwareToHost: vi.fn(async () => ({ ok: true, path: '/tmp/fw.img', bytes: 4 })),
    getLocalFirmwareStatus: vi.fn(async () => ({ ok: true, ready: false })),
    ...over
  }
}

function fakeHost(fw: ReturnType<typeof makeFw>, over: Record<string, unknown> = {}) {
  return {
    isStarted: vi.fn(() => true),
    isDongleUp: vi.fn(() => true),
    uploadIcons: vi.fn(),
    getDevToolsUrlCandidates: vi.fn(() => ['http://192.168.1.1/dev']),
    sendToDongle: vi.fn(async () => true),
    reloadConfigFromDisk: vi.fn(async () => undefined),
    getFirmware: vi.fn(() => fw),
    getApkVer: vi.fn(() => '1.0.0'),
    getDongleFwVersion: vi.fn(() => undefined),
    getBoxInfo: vi.fn(() => undefined),
    emitProjectionEvent: vi.fn(),
    ...over
  }
}

function register(fw = makeFw(), over: Record<string, unknown> = {}) {
  const host = fakeHost(fw, over)
  registerDongleIpc(host as never)
  return { host, fw, h: handlers.get('dongle-fw')! }
}

beforeEach(async () => {
  handlers.clear()
  vi.spyOn(console, 'info').mockImplementation(function () {})
})
afterEach(async () => vi.restoreAllMocks())

describe('dongle ipc — projection-upload-icons', () => {
  test('throws when projection is not started', async () => {
    const { host } = register(makeFw(), { isStarted: vi.fn(() => false) })
    const h = handlers.get('projection-upload-icons')!
    await expect(async () => h(null)).rejects.toThrow(/not started/)
    expect(host.uploadIcons).not.toHaveBeenCalled()
  })

  test('delegates to host.uploadIcons when connected', async () => {
    const { host } = register()
    await handlers.get('projection-upload-icons')!(null)
    expect(host.uploadIcons).toHaveBeenCalled()
  })
})

describe('dongle ipc — dongle-fw check', () => {
  test('shapes a minimal ok result with fallback defaults', async () => {
    const fw = makeFw({ checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: false })) })
    const { h } = register(fw)

    const r = (await h(null, { action: 'check' })) as Record<string, unknown>
    expect(r).toEqual({
      ok: true,
      hasUpdate: false,
      size: 0,
      token: undefined,
      request: undefined,
      raw: {
        err: 0,
        token: undefined,
        ver: undefined,
        size: 0,
        id: undefined,
        notes: undefined,
        msg: undefined,
        error: undefined
      }
    })
  })

  test('prefers top-level fields over raw and falls back to raw values', async () => {
    const fw = makeFw({
      checkForUpdate: vi.fn(async () => ({
        ok: true,
        hasUpdate: true,
        size: 123,
        token: 'tok',
        latestVer: '2.0',
        id: 'fw-1',
        notes: 'notes',
        request: { url: 'u' },
        raw: { err: 'nope', token: 'raw-tok', ver: '1.9', size: '99', msg: 'm', error: 'e' }
      }))
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'check' })) as { raw: Record<string, unknown> }
    expect(r.raw).toEqual({
      err: 0,
      token: 'tok',
      ver: '2.0',
      size: 123,
      id: 'fw-1',
      notes: 'notes',
      msg: 'm',
      error: 'e'
    })
  })

  test('falls back to raw size when the top-level size is missing', async () => {
    const fw = makeFw({
      checkForUpdate: vi.fn(async () => ({
        ok: true,
        hasUpdate: true,
        raw: { err: 2, size: '4096' }
      }))
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'check' })) as { raw: Record<string, unknown> }
    expect(r.raw.err).toBe(2)
    expect(r.raw.size).toBe('4096')
  })
})

describe('dongle ipc — dongle-fw download', () => {
  test('maps a failed check without an error message to the fallback text', async () => {
    const fw = makeFw({ checkForUpdate: vi.fn(async () => ({ ok: false })) })
    const { h, host } = register(fw)

    const r = (await h(null, { action: 'download' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('checkForUpdate failed')
    expect(host.emitProjectionEvent).toHaveBeenCalledWith({
      type: 'fwUpdate',
      stage: 'download:error',
      message: 'checkForUpdate failed'
    })
  })

  test('maps a failed download without an error message to the fallback text', async () => {
    const fw = makeFw({
      checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: true, size: 4 })),
      downloadFirmwareToHost: vi.fn(async () => ({ ok: false, error: '' }))
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'download' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('download failed')
  })

  test('forwards download progress events', async () => {
    const fw = makeFw({
      checkForUpdate: vi.fn(async () => ({ ok: true, hasUpdate: true, size: 4 })),
      downloadFirmwareToHost: vi.fn(
        async (_check: unknown, opts: { onProgress: (p: unknown) => void }) => {
          opts.onProgress({ received: 2, total: 4, percent: 50 })
          return { ok: true, path: '/tmp/fw.img', bytes: 4 }
        }
      )
    })
    const { h, host } = register(fw)

    const r = (await h(null, { action: 'download' })) as Record<string, unknown>
    expect(r.ok).toBe(true)
    expect(host.emitProjectionEvent).toHaveBeenCalledWith({
      type: 'fwUpdate',
      stage: 'download:progress',
      received: 2,
      total: 4,
      percent: 50
    })
  })

  test('stringifies a non-Error throw', async () => {
    const fw = makeFw({
      checkForUpdate: vi.fn(async () => {
        throw 'kaputt'
      })
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'download' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('kaputt')
  })
})

describe('dongle ipc — dongle-fw upload', () => {
  test('maps a failed local status without an error message to the fallback text', async () => {
    const fw = makeFw({ getLocalFirmwareStatus: vi.fn(async () => ({ ok: false })) })
    const { h } = register(fw)

    const r = (await h(null, { action: 'upload' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('Local firmware status failed')
  })

  test('maps a not-ready status without a reason to the fallback text', async () => {
    const fw = makeFw({ getLocalFirmwareStatus: vi.fn(async () => ({ ok: true, ready: false })) })
    const { h } = register(fw)

    const r = (await h(null, { action: 'upload' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('No firmware ready to upload')
  })

  test('stringifies a non-Error throw', async () => {
    const fw = makeFw({
      getLocalFirmwareStatus: vi.fn(async () => {
        throw 'defekt'
      })
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'upload' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('defekt')
  })
})

describe('dongle ipc — dongle-fw status', () => {
  test('maps a failed status with a non-string error to the fallback text', async () => {
    const fw = makeFw({ getLocalFirmwareStatus: vi.fn(async () => ({ ok: false, error: 42 })) })
    const { h } = register(fw)

    const r = (await h(null, { action: 'status' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('Local firmware status failed')
  })

  test('reports a ready firmware without a latestVer as no update', async () => {
    const fw = makeFw({
      getLocalFirmwareStatus: vi.fn(async () => ({
        ok: true,
        ready: true,
        path: '/tmp/fw.img',
        bytes: 10
      }))
    })
    const { h } = register(fw)

    const r = (await h(null, { action: 'status' })) as { raw: Record<string, unknown> }
    expect(r).toMatchObject({ ok: true, hasUpdate: false, size: 10 })
    expect(r.raw).toEqual({ err: 0, ver: undefined, size: 10, msg: 'local:ready' })
  })

  test('unknown actions map to an error response', async () => {
    const { h } = register()
    const r = (await h(null, { action: 'zap' })) as Record<string, unknown>
    expect(r.ok).toBe(false)
    expect(r.error).toBe('Unknown action: zap')
  })
})
