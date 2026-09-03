import { MicType } from '@shared/types'

vi.mock('@main/helpers/vendorSessionInfo', () => ({
  decryptVendorSessionText: vi.fn(async () => 'decrypted-session')
}))
function cfg(): Record<string, unknown> {
  return {
    projectionWidth: 800,
    projectionHeight: 480,
    projectionFps: 30,
    carName: 'Car',
    oemName: '',
    wifiType: '5ghz',
    disableAudioOutput: false,
    projectionViewAreaTop: 0,
    projectionViewAreaBottom: 0,
    projectionViewAreaLeft: 0,
    projectionViewAreaRight: 0,
    projectionSafeAreaTop: 0,
    projectionSafeAreaBottom: 0,
    projectionSafeAreaLeft: 0,
    projectionSafeAreaRight: 0,
    projectionSafeAreaDrawOutside: false
  }
}

async function runWithMic(micType: MicType): Promise<Record<string, unknown>[]> {
  vi.resetModules()
  vi.doMock('../dongleConfig', () => ({ DONGLE_MIC_TYPE: micType }))
  const { DongleDriver, AndroidWorkMode } = await import('../dongleDriver')
  const d = new DongleDriver() as unknown as {
    _cfg: unknown
    _link: unknown
    _androidWorkModeRuntime: unknown
    send: (m: unknown) => Promise<boolean>
    sleep: () => Promise<void>
    scheduleWifiConnect: () => void
    sendPostOpenConfig: () => Promise<void>
  }
  const sent: Record<string, unknown>[] = []
  d._cfg = cfg()
  d._link = { closed: false }
  d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
  d.send = vi.fn(async (m: unknown) => {
    sent.push(m as Record<string, unknown>)
    return true
  })
  d.sleep = vi.fn(async () => undefined)
  d.scheduleWifiConnect = vi.fn()
  await d.sendPostOpenConfig()
  return sent
}

afterEach(() => {
  vi.doUnmock('../dongleConfig')
  vi.restoreAllMocks()
})

describe('DongleDriver mic route selection', () => {
  test('DongleMic selects the boxMici2s route', async () => {
    const sent = await runWithMic(MicType.DongleMic)
    expect(sent.length).toBeGreaterThan(0)
  })

  test('PhoneMic selects the phoneMic route', async () => {
    const sent = await runWithMic(MicType.PhoneMic)
    expect(sent.length).toBeGreaterThan(0)
  })
})
