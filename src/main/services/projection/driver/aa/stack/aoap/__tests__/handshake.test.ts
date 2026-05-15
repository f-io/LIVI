import type { Device } from 'usb'
import {
  ACCESSORY_PIDS,
  AOAP_DESCRIPTION,
  AOAP_MANUFACTURER,
  AOAP_MODEL,
  AOAP_SERIAL,
  AOAP_URI,
  AOAP_VERSION,
  GOOGLE_VID,
  REQ_GET_PROTOCOL,
  REQ_SEND_STRING,
  REQ_START
} from '../constants'
import { isAccessoryMode, probeAaCapable, runAoapHandshake } from '../handshake'

type CtrlCall = {
  bmRequestType: number
  bRequest: number
  wValue: number
  wIndex: number
  data: Buffer | number
}

type FakeDevice = {
  deviceDescriptor: { idVendor: number; idProduct: number }
  open: jest.Mock
  close: jest.Mock
  controlTransfer: jest.Mock
  calls: CtrlCall[]
}

function makeDevice(
  opts: {
    vid?: number
    pid?: number
    protocol?: number
    ctrlError?: Error
    openThrows?: boolean
  } = {}
): FakeDevice {
  const { vid = GOOGLE_VID, pid = 0x4ee1, protocol = 2, ctrlError, openThrows } = opts
  const calls: CtrlCall[] = []
  const controlTransfer = jest.fn(
    (
      bmRequestType: number,
      bRequest: number,
      wValue: number,
      wIndex: number,
      dataOrLength: Buffer | number,
      cb: (err: Error | null, data?: Buffer) => void
    ) => {
      calls.push({ bmRequestType, bRequest, wValue, wIndex, data: dataOrLength })
      if (ctrlError) {
        process.nextTick(() => cb(ctrlError))
        return
      }
      if (bRequest === REQ_GET_PROTOCOL && typeof dataOrLength === 'number') {
        const buf = Buffer.alloc(2)
        buf.writeUInt16LE(protocol, 0)
        process.nextTick(() => cb(null, buf))
        return
      }
      process.nextTick(() => cb(null))
    }
  )

  return {
    deviceDescriptor: { idVendor: vid, idProduct: pid },
    open: jest.fn(() => {
      if (openThrows) throw new Error('open failed')
    }),
    close: jest.fn(),
    controlTransfer,
    calls
  }
}

describe('isAccessoryMode', () => {
  test.each(ACCESSORY_PIDS as readonly number[])('Google VID + PID %s → true', (pid) => {
    const d = makeDevice({ vid: GOOGLE_VID, pid })
    expect(isAccessoryMode(d as unknown as Device)).toBe(true)
  })

  test('non-Google VID → false', () => {
    const d = makeDevice({ vid: 0x1234, pid: 0x4ee1 })
    expect(isAccessoryMode(d as unknown as Device)).toBe(false)
  })

  test('Google VID + non-accessory PID → false', () => {
    const d = makeDevice({ vid: GOOGLE_VID, pid: 0xabcd })
    expect(isAccessoryMode(d as unknown as Device)).toBe(false)
  })
})

describe('probeAaCapable', () => {
  test('opens, asks for AOAP protocol, returns the version, then closes', async () => {
    const d = makeDevice({ protocol: 2 })
    const proto = await probeAaCapable(d as unknown as Device)
    expect(proto).toBe(2)
    expect(d.open).toHaveBeenCalled()
    expect(d.close).toHaveBeenCalled()
    expect(d.calls.find((c) => c.bRequest === REQ_GET_PROTOCOL)).toBeDefined()
  })

  test('returns 0 when the device cannot be opened', async () => {
    const d = makeDevice({ openThrows: true })
    expect(await probeAaCapable(d as unknown as Device)).toBe(0)
    expect(d.close).not.toHaveBeenCalled()
  })

  test('returns 0 when the control transfer fails', async () => {
    const d = makeDevice({ ctrlError: new Error('pipe stall') })
    expect(await probeAaCapable(d as unknown as Device)).toBe(0)
    expect(d.close).toHaveBeenCalled()
  })

  test('returns 0 when protocol is < 1', async () => {
    const d = makeDevice({ protocol: 0 })
    expect(await probeAaCapable(d as unknown as Device)).toBe(0)
  })
})

describe('runAoapHandshake', () => {
  test('returns immediately when the device is already in accessory mode', async () => {
    const d = makeDevice({ vid: GOOGLE_VID, pid: 0x2d00 })
    await runAoapHandshake(d as unknown as Device)
    expect(d.controlTransfer).not.toHaveBeenCalled()
  })

  test('walks the full sequence: getProtocol → 6× sendString → start', async () => {
    const d = makeDevice({ pid: 0x4ee1, protocol: 2 })
    await runAoapHandshake(d as unknown as Device)

    const sendStrings = d.calls.filter((c) => c.bRequest === REQ_SEND_STRING)
    expect(sendStrings).toHaveLength(6)
    expect(d.calls.find((c) => c.bRequest === REQ_GET_PROTOCOL)).toBeDefined()
    expect(d.calls.find((c) => c.bRequest === REQ_START)).toBeDefined()
  })

  test('sendString passes wIndex = string-id and a NUL-terminated UTF-8 buffer', async () => {
    const d = makeDevice({ pid: 0x4ee1, protocol: 2 })
    await runAoapHandshake(d as unknown as Device)
    const expectedStrings = [
      AOAP_MANUFACTURER,
      AOAP_MODEL,
      AOAP_DESCRIPTION,
      AOAP_VERSION,
      AOAP_URI,
      AOAP_SERIAL
    ]
    const sends = d.calls.filter((c) => c.bRequest === REQ_SEND_STRING)
    sends.forEach((s, i) => {
      const buf = s.data as Buffer
      expect(buf[buf.length - 1]).toBe(0) // NUL terminator
      expect(buf.subarray(0, buf.length - 1).toString('utf8')).toBe(expectedStrings[i])
      expect(s.wIndex).toBe(i)
    })
  })

  test('rejects when protocol < 1', async () => {
    const d = makeDevice({ pid: 0x4ee1, protocol: 0 })
    await expect(runAoapHandshake(d as unknown as Device)).rejects.toThrow('not supported')
  })

  test('propagates a control-transfer error', async () => {
    const d = makeDevice({ pid: 0x4ee1, ctrlError: new Error('boom') })
    await expect(runAoapHandshake(d as unknown as Device)).rejects.toThrow('boom')
  })

  test('times out when the control transfer never completes', async () => {
    jest.useFakeTimers()
    const calls: CtrlCall[] = []
    const stuck = jest.fn(() => calls)
    const d: FakeDevice = {
      deviceDescriptor: { idVendor: GOOGLE_VID, idProduct: 0x4ee1 },
      open: jest.fn(),
      close: jest.fn(),
      controlTransfer: stuck,
      calls
    }
    const p = runAoapHandshake(d as unknown as Device)
    jest.advanceTimersByTime(2_500)
    await expect(p).rejects.toThrow(/timeout/)
    jest.useRealTimers()
  })
})
