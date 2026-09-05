import { getSecondaryWindow } from '@main/window/secondaryWindows'
import type { Config } from '@shared/types'
import type { WebContents } from 'electron'
import { GstVideo } from '../../../video/GstVideo'
import { classifyNal } from '../../../video/keyframe'
import { VideoPlaneManager, type VideoPlaneManagerDeps } from '../VideoPlaneManager'

vi.mock('../../../video/GstVideo', () => ({
  GstVideo: vi.fn().mockImplementation(function () {
    return {
      dispose: vi.fn(),
      setVisible: vi.fn(),
      setCodecData: vi.fn(),
      prepare: vi.fn(),
      push: vi.fn(),
      setContentRegion: vi.fn()
    }
  })
}))

vi.mock('../../../video/gstHost', () => ({
  VIDEO_PLANE_MAIN: 101,
  clusterPlaneId: vi.fn(() => 202)
}))

vi.mock('../../../video/keyframe', () => ({
  classifyNal: vi.fn(() => 'keyframe')
}))

vi.mock('@main/window/secondaryWindows', () => ({
  getSecondaryWindow: vi.fn()
}))

const gstMock = vi.mocked(GstVideo)
const classifyMock = vi.mocked(classifyNal)
const secondaryMock = vi.mocked(getSecondaryWindow)

type PlaneMock = {
  dispose: ReturnType<typeof vi.fn>
  setVisible: ReturnType<typeof vi.fn>
  setCodecData: ReturnType<typeof vi.fn>
  prepare: ReturnType<typeof vi.fn>
  push: ReturnType<typeof vi.fn>
  setContentRegion: ReturnType<typeof vi.fn>
}

function planes(): PlaneMock[] {
  return gstMock.mock.results.map((r) => r.value as PlaneMock)
}

function mkCfg(over: Record<string, unknown> = {}): Config {
  return over as unknown as Config
}

function mkMgr(over: Partial<VideoPlaneManagerDeps> = {}): {
  mgr: VideoPlaneManager
  deps: {
    getWebContents: ReturnType<typeof vi.fn>
    getConfig: ReturnType<typeof vi.fn>
    emit: ReturnType<typeof vi.fn>
    getMainVideoSize: ReturnType<typeof vi.fn>
    getClusterVideoSize: ReturnType<typeof vi.fn>
  }
  wc: { isDestroyed: ReturnType<typeof vi.fn> }
} {
  const wc = { isDestroyed: vi.fn(() => false) }
  const deps = {
    getWebContents: vi.fn(() => wc as unknown as WebContents),
    getConfig: vi.fn(() => mkCfg()),
    emit: vi.fn(),
    getMainVideoSize: vi.fn(() => ({ width: 0, height: 0 })),
    getClusterVideoSize: vi.fn(() => ({ width: 0, height: 0 })),
    ...over
  }
  return { mgr: new VideoPlaneManager(deps as unknown as VideoPlaneManagerDeps), deps, wc }
}

const CLUSTER_CFG = mkCfg({ dashboards: { dash3: { main: true, dash: true } } })

function mkSecondary(): { isDestroyed: () => boolean; webContents: Record<string, never> } {
  return { isDestroyed: () => false, webContents: {} }
}

describe('VideoPlaneManager', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    classifyMock.mockReturnValue('keyframe')
  })

  describe('main plane', () => {
    test('primeMain builds the main plane and prepares it', () => {
      const { mgr } = mkMgr()
      expect(mgr.primeMain()).toBe(true)
      expect(planes()[0].prepare).toHaveBeenCalled()
    })

    test('primeMain returns false without usable webContents', () => {
      const { mgr, wc } = mkMgr()
      wc.isDestroyed.mockReturnValue(true)
      expect(mgr.primeMain()).toBe(false)
      expect(planes()).toHaveLength(0)
    })

    test('prepareMain creates the plane via the native-config path', () => {
      const { mgr } = mkMgr()
      const atom = Buffer.from('atom')

      expect(mgr.prepareMain('h265', atom)).toBe(true)
      expect(mgr.getMainCodec()).toBe('h265')
      expect(gstMock).toHaveBeenCalledWith(expect.anything(), 'main', 'main', 101)
      expect(planes()[0].prepare).toHaveBeenCalledWith('h265', atom)
      expect(planes()[0].setVisible).toHaveBeenCalledWith(true)

      expect(mgr.prepareMain('h264', atom)).toBe(false)
      expect(gstMock).toHaveBeenCalledTimes(1)
    })

    test('prepareMain fails without usable webContents', () => {
      const { mgr, wc } = mkMgr()
      wc.isDestroyed.mockReturnValue(true)
      expect(mgr.prepareMain('h264', Buffer.alloc(0))).toBe(false)

      const { mgr: mgr2 } = mkMgr({ getWebContents: vi.fn(() => null) })
      expect(mgr2.prepareMain('h264', Buffer.alloc(0))).toBe(false)
      expect(gstMock).not.toHaveBeenCalled()
    })

    test('setVideoVisible drives the plane when it exists', () => {
      const { mgr } = mkMgr()
      mgr.setVideoVisible(false)

      mgr.primeMain()
      expect(planes()[0].setVisible).toHaveBeenCalledWith(false)

      mgr.setVideoVisible(true)
      expect(planes()[0].setVisible).toHaveBeenCalledWith(true)
    })

    test('updateMainCrop centers the negotiated content and clears without sizes', () => {
      const { mgr, deps } = mkMgr({
        getMainVideoSize: vi.fn(() => ({ width: 1920, height: 1080 })),
        getConfig: vi.fn(() => mkCfg({ projectionWidth: 960, projectionHeight: 1080 }))
      })
      mgr.primeMain()

      mgr.updateMainCrop()
      expect(planes()[0].setContentRegion).toHaveBeenLastCalledWith(480, 0, 960, 1080, 1920, 1080)

      deps.getConfig.mockReturnValue(mkCfg())
      mgr.updateMainCrop()
      expect(planes()[0].setContentRegion).toHaveBeenLastCalledWith(0, 0, 0, 0, 0, 0)
    })

    test('updateMainCrop without a plane only stores the crop', () => {
      const { mgr } = mkMgr()

      mgr.updateMainCrop()

      expect(gstMock).not.toHaveBeenCalled()
    })

    test('a stored crop is applied when the plane spawns', () => {
      const { mgr } = mkMgr({
        getMainVideoSize: vi.fn(() => ({ width: 1920, height: 1080 })),
        getConfig: vi.fn(() => mkCfg({ projectionWidth: 1920, projectionHeight: 1080 }))
      })
      mgr.updateMainCrop()

      mgr.primeMain()

      expect(planes()[0].setContentRegion).toHaveBeenCalledWith(0, 0, 1920, 1080, 1920, 1080)
    })
  })

  describe('cluster planes', () => {
    test('prepareClusters creates one plane per reachable target screen', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      const atom = Buffer.from('atom')

      expect(mgr.prepareClusters('h265', atom)).toBe(true)
      expect(gstMock).toHaveBeenCalledTimes(2)
      expect(planes()[0].setVisible).toHaveBeenCalledWith(false)
      expect(planes()[1].setVisible).toHaveBeenCalledWith(true)
      expect(planes()[0].prepare).toHaveBeenCalledWith('h265', atom)

      expect(mgr.prepareClusters('h264', atom)).toBe(false)
      expect(gstMock).toHaveBeenCalledTimes(2)
    })

    test('primeClusters waits for the real codec before building a plane', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)

      mgr.primeClusters()
      expect(gstMock).not.toHaveBeenCalled()

      mgr.setClusterCodec('h265')
      mgr.primeClusters()
      expect(gstMock).toHaveBeenCalledTimes(2)
      expect(planes()[0].prepare).toHaveBeenCalledWith('h265')
    })

    test('primeClusters builds a plane per screen once the codec is known', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setClusterCodec('h265')
      mgr.setClusterCodecData(Buffer.from([1, 2]))
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(gstMock).toHaveBeenCalledTimes(2)
      expect(planes()[0].setVisible).toHaveBeenCalled()
      expect(planes()[0].setCodecData).toHaveBeenCalledWith(Buffer.from([1, 2]))
    })

    test('primeClusters only prepares a plane that already exists', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setClusterCodec('h265')
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      const built = gstMock.mock.calls.length
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(gstMock.mock.calls.length).toBe(built)
      expect(planes()[0].prepare).toHaveBeenCalledTimes(2)
    })

    test('primeClusters skips a screen whose window is gone', () => {
      const { mgr } = mkMgr({
        getConfig: vi.fn(() => mkCfg({ dashboards: { dash4: { dash: true } } }))
      })
      secondaryMock.mockReturnValue({ isDestroyed: () => true, webContents: {} } as never)
      mgr.setClusterCodec('h265')
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(gstMock).not.toHaveBeenCalled()
    })

    test('ensureClusterPlanes adds a plane for a screen that appears later', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      const atom = Buffer.from('atom')

      expect(mgr.ensureClusterPlanes()).toBe(false)

      secondaryMock.mockReturnValue(null as never)
      expect(mgr.prepareClusters('h265', atom)).toBe(true)
      const before = gstMock.mock.calls.length

      secondaryMock.mockReturnValue(mkSecondary() as never)
      expect(mgr.ensureClusterPlanes()).toBe(true)
      expect(gstMock.mock.calls.length).toBe(before + 1)
      expect(planes().at(-1)?.prepare).toHaveBeenCalledWith('h265', atom)

      expect(mgr.ensureClusterPlanes()).toBe(false)
    })

    test('prepareClusters skips screens without a live window', () => {
      const { mgr } = mkMgr({
        getWebContents: vi.fn(() => null),
        getConfig: vi.fn(() =>
          mkCfg({ dashboards: { dash4: { main: true, dash: true, aux: true } } })
        )
      })
      secondaryMock.mockImplementation(
        (role: string) =>
          (role === 'dash' ? { isDestroyed: () => true, webContents: {} } : undefined) as never
      )

      expect(mgr.prepareClusters('h264', Buffer.alloc(0))).toBe(false)
      expect(gstMock).not.toHaveBeenCalled()
    })

    test('setClusterVisible drives only the main-screen plane', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setClusterVisible(true)

      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(planes()[0].setVisible).toHaveBeenCalledWith(true)
      expect(planes()[1].setVisible).toHaveBeenCalledWith(true)

      mgr.setClusterVisible(false)
      expect(planes()[0].setVisible).toHaveBeenLastCalledWith(false)
      expect(planes()[1].setVisible).toHaveBeenCalledTimes(1)
    })

    test('cluster crops center the content or collapse to zero without sizes', () => {
      const { mgr, deps } = mkMgr({
        getConfig: vi.fn(() => mkCfg({ ...CLUSTER_CFG, clusterWidth: 960, clusterHeight: 540 })),
        getClusterVideoSize: vi.fn(() => ({ width: 1920, height: 1080 }))
      })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(planes()[0].setContentRegion).toHaveBeenCalledWith(0, 0, 1920, 1080, 1920, 1080)

      deps.getConfig.mockReturnValue(CLUSTER_CFG)
      mgr.recropAllClusters()
      expect(planes()[0].setContentRegion).toHaveBeenLastCalledWith(0, 0, 0, 0, 0, 0)
      expect(planes()[1].setContentRegion).toHaveBeenLastCalledWith(0, 0, 0, 0, 0, 0)
    })

    test('retainScreens disposes planes for screens no longer targeted', () => {
      const { mgr, deps } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(gstMock).toHaveBeenCalledTimes(2)

      deps.getConfig.mockReturnValue(mkCfg({ dashboards: { dash3: { main: true } } }))
      mgr.retainScreens()

      expect(planes()[1].dispose).toHaveBeenCalledTimes(1)
      expect(planes()[0].dispose).not.toHaveBeenCalled()

      mgr.setClusterCodec('h264')
      mgr.primeClusters()
      expect(gstMock).toHaveBeenCalledTimes(2)
    })
  })

  describe('lifecycle and codec state', () => {
    test('dispose is safe without planes and resets codec state', () => {
      const { mgr } = mkMgr()

      mgr.dispose()

      expect(mgr.getMainCodec()).toBe('h264')
    })

    test('dispose tears down main and cluster planes', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      mgr.setMainCodec('av1')
      mgr.primeMain()
      mgr.setClusterCodec('h264')
      mgr.primeClusters()

      mgr.dispose()

      for (const plane of planes()) expect(plane.dispose).toHaveBeenCalledTimes(1)
      expect(mgr.getMainCodec()).toBe('h264')
    })

    test('restoreCodecs seeds only the provided codecs and keeps the raw codec data', () => {
      const { mgr } = mkMgr({ getConfig: vi.fn(() => CLUSTER_CFG) })
      secondaryMock.mockReturnValue(mkSecondary() as never)
      const mainAtom = Buffer.from('m')

      mgr.restoreCodecs('h265', 'vp9', mainAtom, null)
      expect(mgr.getMainCodec()).toBe('h265')

      mgr.primeMain()
      expect(planes()[0].setCodecData).toHaveBeenCalledWith(mainAtom)
      expect(planes()[0].prepare).toHaveBeenCalledWith('h265')

      mgr.primeClusters()
      expect(planes()[1].setCodecData).not.toHaveBeenCalled()
      expect(planes()[1].prepare).toHaveBeenCalledWith('vp9')

      mgr.restoreCodecs(undefined, undefined, null, null)
      expect(mgr.getMainCodec()).toBe('h265')
    })
  })
})
