describe('USBWorker', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  test('throws if parentPort is missing', () => {
    jest.doMock('worker_threads', () => ({ parentPort: null }))

    expect(() => {
      jest.isolateModules(() => {
        require('@main/services/usb/USBWorker')
      })
    }).toThrow('No parent port found')
  })

  test('posts connected status on check-dongle when helper finds device', () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(() => ({
        deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1520 }
      }))
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    expect(cb).toBeDefined()

    cb('check-dongle')

    expect(postMessage).toHaveBeenCalledWith({
      type: 'dongle-status',
      connected: true,
      vendorId: 0x1314,
      productId: 0x1520
    })
  })

  test('posts disconnected status on check-dongle when no device', () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(() => null)
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    cb('check-dongle')

    expect(postMessage).toHaveBeenCalledWith({ type: 'dongle-status', connected: false })
  })

  test('ignores unknown worker messages', () => {
    const on = jest.fn()
    const postMessage = jest.fn()

    jest.doMock('worker_threads', () => ({
      parentPort: { on, postMessage }
    }))

    jest.doMock('@main/services/usb/helpers', () => ({
      findDongle: jest.fn(() => null)
    }))

    jest.isolateModules(() => {
      require('@main/services/usb/USBWorker')
    })

    const cb = on.mock.calls.find(([evt]: [string]) => evt === 'message')?.[1]
    cb('noop')

    expect(postMessage).not.toHaveBeenCalled()
  })
})
