describe('USBWorker', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  const flush = () => new Promise((r) => setImmediate(r))

  test('throws if parentPort is missing', () => {
    jest.doMock('worker_threads', () => ({ parentPort: null }))

    expect(() => {
      jest.isolateModules(() => {
        require('@main/services/usb/USBWorker')
      })
    }).toThrow('No parent port found')
  })

  test('posts connected status on check-dongle when helper finds device', async () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(async () => ({
        vendorId: 0x1314,
        productId: 0x1520
      }))
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    expect(cb).toBeDefined()

    cb('check-dongle')
    await flush()

    expect(postMessage).toHaveBeenCalledWith({
      type: 'dongle-status',
      connected: true,
      vendorId: 0x1314,
      productId: 0x1520
    })
  })

  test('posts disconnected status on check-dongle when no device', async () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(async () => null)
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    cb('check-dongle')
    await flush()

    expect(postMessage).toHaveBeenCalledWith({ type: 'dongle-status', connected: false })
  })

  test('ignores unknown worker messages', async () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(async () => null)
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    cb('noop')
    await flush()

    expect(postMessage).not.toHaveBeenCalled()
  })
})
