import { AndroidWorkMode, DongleDriver } from '@main/services/projection/driver/DongleDriver'
import { SendCommand } from '@main/services/projection/messages/sendable'

describe('DongleDriver core behavior', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  test('emitDongleInfoIfChanged emits only when payload key changes', () => {
    const d = new DongleDriver() as any
    const onInfo = jest.fn()
    d.on('dongle-info', onInfo)

    d._dongleFwVersion = '1.0.0'
    d._boxInfo = { productType: 'A15W' }

    d.emitDongleInfoIfChanged()
    d.emitDongleInfoIfChanged()

    expect(onInfo).toHaveBeenCalledTimes(1)
    expect(onInfo).toHaveBeenCalledWith({
      dongleFwVersion: '1.0.0',
      boxInfo: { productType: 'A15W' }
    })
  })

  test('scheduleWifiConnect debounces timers and sends wifiConnect command once', async () => {
    const d = new DongleDriver() as any
    d.send = jest.fn(async () => true)

    d.scheduleWifiConnect(100)
    d.scheduleWifiConnect(200)

    jest.advanceTimersByTime(200)
    await Promise.resolve()

    expect(d.send).toHaveBeenCalledTimes(1)
    expect(d.send.mock.calls[0][0]).toBeInstanceOf(SendCommand)
  })

  test('applyAndroidWorkMode no-ops when mode unchanged', async () => {
    const d = new DongleDriver() as any
    d._androidWorkModeRuntime = AndroidWorkMode.AndroidAuto
    d.send = jest.fn(async () => true)

    await d.applyAndroidWorkMode(AndroidWorkMode.AndroidAuto)

    expect(d.send).not.toHaveBeenCalled()
  })

  test('applyAndroidWorkMode updates mode and sends config + wifi enable', async () => {
    const d = new DongleDriver() as any
    d._androidWorkModeRuntime = AndroidWorkMode.Off
    d.send = jest.fn(async () => true)

    await d.applyAndroidWorkMode(AndroidWorkMode.AndroidAuto)

    expect(d._androidWorkModeRuntime).toBe(AndroidWorkMode.AndroidAuto)
    expect(d.send).toHaveBeenCalledTimes(2)
  })
})
