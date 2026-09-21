const { dongleStatus } = vi.hoisted(() => ({ dongleStatus: vi.fn() }))
vi.mock('@main/services/link/dongleAp', () => ({ dongleStatus, noteDongleStatus: vi.fn() }))

const { broadcastToRenderers } = vi.hoisted(() => ({ broadcastToRenderers: vi.fn() }))
vi.mock('@main/window/broadcast', () => ({ broadcastToRenderers }))

const POLL_MS = 1500

let startLinkSpeedMonitor: () => void
let stopLinkSpeedMonitor: () => void

// Each test gets its own module instance: the monitor keeps the interval, the in-flight flag and
// the previous sample in module scope.
beforeEach(async () => {
  vi.resetModules()
  vi.useFakeTimers()
  dongleStatus.mockReset()
  broadcastToRenderers.mockReset()
  ;({ startLinkSpeedMonitor, stopLinkSpeedMonitor } = await import('../linkSpeed'))
})

afterEach(() => {
  stopLinkSpeedMonitor()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

/** Fires the poll interval `times` times, letting each sample's promises settle. */
async function poll(times = 1): Promise<void> {
  for (let i = 0; i < times; i++) await vi.advanceTimersByTimeAsync(POLL_MS)
}

/** The last thing pushed to the renderers. */
function broadcast(): unknown {
  const call = broadcastToRenderers.mock.calls.at(-1)
  expect(call?.[0]).toBe('link-speed')
  return call?.[1]
}

describe('link speed monitor', () => {
  it('reports the PHY rates and no throughput on the first sample', async () => {
    dongleStatus.mockResolvedValue({
      downbytes: '1000',
      upbytes: '500',
      downrate: '866',
      uprate: '780'
    })
    startLinkSpeedMonitor()
    await poll()

    expect(broadcast()).toEqual({ downMbps: 0, upMbps: 0, downRate: 866, upRate: 780 })
  })

  it('derives throughput from the byte delta between two polls', async () => {
    dongleStatus
      .mockResolvedValueOnce({ downbytes: '0', upbytes: '0', downrate: '866', uprate: '780' })
      .mockResolvedValueOnce({
        downbytes: '1000000',
        upbytes: '500000',
        downrate: '866',
        uprate: '780'
      })
    startLinkSpeedMonitor()
    await poll(2)

    // 1 MB and 0.5 MB over 1.5 s, one decimal.
    expect(broadcast()).toEqual({ downMbps: 5.3, upMbps: 2.7, downRate: 866, upRate: 780 })
  })

  it('ignores a counter reset instead of reporting a negative rate', async () => {
    dongleStatus
      .mockResolvedValueOnce({ downbytes: '9000000', upbytes: '9000000' })
      .mockResolvedValueOnce({ downbytes: '10', upbytes: '10' })
    startLinkSpeedMonitor()
    await poll(2)

    expect(broadcast()).toMatchObject({ downMbps: 0, upMbps: 0 })
  })

  it('reports no rate when two samples land on the same millisecond', async () => {
    vi.spyOn(Date, 'now').mockReturnValue(1_000)
    dongleStatus
      .mockResolvedValueOnce({ downbytes: '0', upbytes: '0' })
      .mockResolvedValueOnce({ downbytes: '5000000', upbytes: '5000000' })
    startLinkSpeedMonitor()
    await poll(2)

    expect(broadcast()).toMatchObject({ downMbps: 0, upMbps: 0 })
  })

  it('reports null while the dongle is not reachable', async () => {
    dongleStatus.mockResolvedValue(null)
    startLinkSpeedMonitor()
    await poll()

    expect(broadcastToRenderers).toHaveBeenCalledWith('link-speed', null)
  })

  it('forgets the previous sample across a gap, so the next reading is not a burst', async () => {
    dongleStatus
      .mockResolvedValueOnce({ downbytes: '0', upbytes: '0' })
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({ downbytes: '9000000', upbytes: '9000000' })
    startLinkSpeedMonitor()
    await poll(3)

    expect(broadcast()).toMatchObject({ downMbps: 0, upMbps: 0 })
  })

  it('treats unparsable counters and rates as no reading', async () => {
    dongleStatus.mockResolvedValue({
      downbytes: 'n/a',
      upbytes: 'n/a',
      downrate: 'x',
      uprate: ''
    })
    startLinkSpeedMonitor()
    await poll(2)

    expect(broadcast()).toEqual({ downMbps: 0, upMbps: 0, downRate: 0, upRate: 0 })
  })

  it('skips a tick while the previous sample is still in flight', async () => {
    let release: (value: unknown) => void = () => {}
    dongleStatus.mockImplementation(
      () =>
        new Promise((resolve) => {
          release = resolve
        })
    )
    startLinkSpeedMonitor()
    await poll(3)

    expect(dongleStatus).toHaveBeenCalledTimes(1)

    release(null)
    await vi.advanceTimersByTimeAsync(0)
  })

  it('starting twice keeps a single interval', async () => {
    dongleStatus.mockResolvedValue(null)
    startLinkSpeedMonitor()
    startLinkSpeedMonitor()
    await poll()

    expect(dongleStatus).toHaveBeenCalledTimes(1)
  })

  it('stops polling once it is stopped', async () => {
    dongleStatus.mockResolvedValue(null)
    startLinkSpeedMonitor()
    await poll()
    stopLinkSpeedMonitor()
    await poll(2)

    expect(dongleStatus).toHaveBeenCalledTimes(1)
  })

  it('stopping a monitor that never started is a no-op', () => {
    expect(() => stopLinkSpeedMonitor()).not.toThrow()
    expect(broadcastToRenderers).not.toHaveBeenCalled()
  })
})
