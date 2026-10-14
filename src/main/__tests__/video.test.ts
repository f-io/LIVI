import { app, type BrowserWindow } from 'electron'

vi.mock('@shared/core/link', () => ({ coreSocketPath: () => '/run/livi/core.sock' }))

const realPlatform = process.platform
const onPlatform = (platform: string) =>
  Object.defineProperty(process, 'platform', { value: platform })
const GST_VARS = ['GST_PLUGIN_SYSTEM_PATH', 'GST_PLUGIN_PATH', 'GST_PLUGIN_SCANNER'] as const
const appMock = app as unknown as { isPackaged: boolean; getAppPath: () => string }

type Addon = { serve: ReturnType<typeof vi.fn>; setWindow: ReturnType<typeof vi.fn> }

function addonThatServes(serves = true): Addon {
  const addon = { serve: vi.fn(() => serves), setWindow: vi.fn() }
  vi.spyOn(process, 'dlopen').mockImplementation((mod: { exports: unknown }) => {
    mod.exports = addon
  })
  return addon
}

const handle = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8])
const window = (destroyed = false) =>
  ({
    isDestroyed: () => destroyed,
    getNativeWindowHandle: () => handle
  }) as unknown as BrowserWindow

async function load() {
  vi.resetModules()
  return import('@main/video')
}

beforeEach(() => {
  onPlatform('darwin')
  appMock.isPackaged = false
  appMock.getAppPath = () => '/repo'
  Object.defineProperty(process, 'resourcesPath', { value: '/LIVI.app/Res', configurable: true })
  for (const v of GST_VARS) delete process.env[v]
})

afterEach(() => {
  vi.restoreAllMocks()
  onPlatform(realPlatform)
  for (const v of GST_VARS) delete process.env[v]
})

describe('video', () => {
  test('gst-host hands the planes over next to core', async () => {
    const { uiPlanesPath } = await load()
    expect(uiPlanesPath()).toBe('/run/livi/ui-planes.sock')
  })

  test('other platforms have a compositor and leave the addon alone', async () => {
    onPlatform('linux')
    const addon = addonThatServes()
    const { startVideo, drawVideoIn } = await load()
    startVideo()
    drawVideoIn('main', window())
    expect(process.dlopen).not.toHaveBeenCalled()
    expect(addon.setWindow).not.toHaveBeenCalled()
  })

  test('a checkout loads its build and listens where the compositor would', async () => {
    const addon = addonThatServes()
    const { startVideo } = await load()
    startVideo()
    expect(process.dlopen).toHaveBeenCalledWith(
      expect.anything(),
      '/repo/native/livi-gst-video/build/Release/gst_video.node'
    )
    expect(addon.serve).toHaveBeenCalledWith(
      '/run/livi/compositor.ctrl',
      '/run/livi/ui-planes.sock'
    )
    expect(process.env.GST_PLUGIN_PATH).toBeUndefined()
  })

  test('the app loads its own copy on the GStreamer it ships', async () => {
    appMock.isPackaged = true
    addonThatServes()
    const { startVideo } = await load()
    startVideo()
    expect(process.dlopen).toHaveBeenCalledWith(
      expect.anything(),
      '/LIVI.app/Res/video/gst_video.node'
    )
    expect(process.env.GST_PLUGIN_SYSTEM_PATH).toBe('')
    expect(process.env.GST_PLUGIN_PATH).toBe(
      '/LIVI.app/Res/gstreamer/macos-arm64/lib/gstreamer-1.0'
    )
    expect(process.env.GST_PLUGIN_SCANNER).toBe(
      '/LIVI.app/Res/gstreamer/macos-arm64/libexec/gstreamer-1.0/gst-plugin-scanner'
    )
  })

  test('each screen tells the addon its window, and when it is gone', async () => {
    const addon = addonThatServes()
    const { startVideo, drawVideoIn } = await load()
    startVideo()
    drawVideoIn('main', window())
    drawVideoIn('cluster', null)
    drawVideoIn('aux', window(true))
    expect(addon.setWindow.mock.calls).toEqual([
      ['main', handle],
      ['cluster', null],
      ['aux', null]
    ])
  })

  test('an addon that does not load leaves the app without video', async () => {
    vi.spyOn(process, 'dlopen').mockImplementation(() => {
      throw new Error('image not found')
    })
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const { startVideo, drawVideoIn } = await load()
    startVideo()
    drawVideoIn('main', window())
    expect(error).toHaveBeenCalledWith(expect.stringContaining('image not found'))
  })

  test('an addon that cannot listen draws nothing', async () => {
    const addon = addonThatServes(false)
    const { startVideo, drawVideoIn } = await load()
    startVideo()
    drawVideoIn('main', window())
    expect(addon.setWindow).not.toHaveBeenCalled()
  })
})
