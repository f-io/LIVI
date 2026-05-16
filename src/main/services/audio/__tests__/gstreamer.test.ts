jest.mock('electron', () => ({
  app: { isPackaged: false, getAppPath: () => '/repo' }
}))

import {
  audioDeviceProp,
  audioSinkElement,
  audioSourceElement,
  gstEnv,
  resolveBinary,
  resolveGStreamerRoot
} from '../gstreamer'

describe('gstreamer helpers — platform-correct element + prop names', () => {
  const origPlatform = process.platform
  const setPlatform = (p: NodeJS.Platform) =>
    Object.defineProperty(process, 'platform', { value: p, configurable: true })
  afterEach(() => setPlatform(origPlatform))

  test('linux uses pulsesink / pulsesrc / device', () => {
    setPlatform('linux')
    expect(audioSinkElement()).toBe('pulsesink')
    expect(audioSourceElement()).toBe('pulsesrc')
    expect(audioDeviceProp()).toBe('device')
  })

  test('darwin uses osxaudiosink / osxaudiosrc / unique-id (GStreamer 1.28+)', () => {
    setPlatform('darwin')
    expect(audioSinkElement()).toBe('osxaudiosink')
    expect(audioSourceElement()).toBe('osxaudiosrc')
    expect(audioDeviceProp()).toBe('unique-id')
  })

  test('win32 uses wasapisink / wasapisrc / device-name', () => {
    setPlatform('win32')
    expect(audioSinkElement()).toBe('wasapisink')
    expect(audioSourceElement()).toBe('wasapisrc')
    expect(audioDeviceProp()).toBe('device-name')
  })
})

describe('gstEnv', () => {
  const origPlatform = process.platform
  const setPlatform = (p: NodeJS.Platform) =>
    Object.defineProperty(process, 'platform', { value: p, configurable: true })
  afterEach(() => setPlatform(origPlatform))

  test('linux sets LD_LIBRARY_PATH', () => {
    setPlatform('linux')
    const env = gstEnv('/opt/gst')
    expect(env.LD_LIBRARY_PATH).toBe('/opt/gst/lib')
    expect(env.GST_PLUGIN_PATH).toBe('/opt/gst/lib/gstreamer-1.0')
    expect(env.GST_PLUGIN_SYSTEM_PATH).toBe('')
  })

  test('darwin sets DYLD_LIBRARY_PATH', () => {
    setPlatform('darwin')
    const env = gstEnv('/opt/gst')
    expect(env.DYLD_LIBRARY_PATH).toBe('/opt/gst/lib')
  })

  test('win32 prepends bin dir to PATH with a semicolon', () => {
    setPlatform('win32')
    const env = gstEnv('C:/gst')
    // PATH separator on win32 is ';'. The bin path itself uses whatever
    // path.join uses on the host running the test (forward slashes on
    // mac/linux jest runs), so just assert the separator + bin marker.
    expect(env.PATH).toMatch(/bin;/)
    expect(env.PATH).toContain('C:/gst')
  })
})

describe('resolveGStreamerRoot / resolveBinary', () => {
  const origPlatform = process.platform
  const origArch = process.arch
  const setPlatform = (p: NodeJS.Platform) =>
    Object.defineProperty(process, 'platform', { value: p, configurable: true })
  const setArch = (a: NodeJS.Architecture) =>
    Object.defineProperty(process, 'arch', { value: a, configurable: true })
  afterEach(() => {
    setPlatform(origPlatform)
    setArch(origArch)
  })

  test('unsupported platform returns null', () => {
    setPlatform('freebsd' as NodeJS.Platform)
    expect(resolveGStreamerRoot()).toBeNull()
  })

  test('unsupported arch returns null on supported platform', () => {
    setPlatform('linux')
    setArch('ia32' as NodeJS.Architecture)
    expect(resolveGStreamerRoot()).toBeNull()
  })

  test('resolveBinary returns null when root cannot be resolved', () => {
    setPlatform('freebsd' as NodeJS.Platform)
    expect(resolveBinary('gst-launch-1.0')).toBeNull()
    expect(resolveBinary('gst-device-monitor-1.0')).toBeNull()
  })
})
