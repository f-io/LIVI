import {
  SendBluetoothPairedList,
  SendCommand,
  SendTmpFile,
  boxTmpPath
} from '@main/services/projection/messages/sendable'
import { MessageType } from '@main/services/projection/messages/common'

describe('sendable messages', () => {
  test('SendCommand serialises message header + mapped payload', () => {
    const msg = new SendCommand('frame')
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.Command)
    expect(buf.readUInt32LE(16)).toBeGreaterThanOrEqual(0)
  })

  test('SendBluetoothPairedList appends NUL terminator', () => {
    const msg = new SendBluetoothPairedList('Device A')
    const payload = msg.getPayload()
    expect(payload[payload.length - 1]).toBe(0)
  })

  test('boxTmpPath sanitizes path and defaults empty names', () => {
    expect(boxTmpPath('a/b/c.img')).toBe('/tmp/c.img')
    expect(boxTmpPath('   ')).toBe('/tmp/update.img')
  })

  test('SendTmpFile always targets /tmp/<file>', () => {
    const msg = new SendTmpFile(Buffer.from([1, 2, 3]), '/weird/path/fw.img')
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')

    expect(name).toBe('/tmp/fw.img')
  })
})
