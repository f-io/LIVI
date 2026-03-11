import { createCipheriv } from 'node:crypto'
import {
  decodeVendorSessionAscii,
  decodeVendorSessionBase64,
  decryptVendorSessionInfo,
  decryptVendorSessionText
} from '@main/helpers/vendorSessionInfo'

const VENDOR_SESSION_KEY = Buffer.from('573245433158314e625a35385458746e', 'hex')

describe('vendorSessionInfo helpers', () => {
  test('decodeVendorSessionAscii removes trailing null bytes', () => {
    const raw = Buffer.from('YWJjZA==\0\0\0', 'ascii')
    expect(decodeVendorSessionAscii(raw)).toBe('YWJjZA==')
  })

  test('decodeVendorSessionBase64 decodes base64 ascii to buffer', () => {
    const decoded = decodeVendorSessionBase64('YWJjZA==')
    expect(decoded.equals(Buffer.from('abcd'))).toBe(true)
  })

  test('decryptVendorSessionInfo throws when payload is too short', async () => {
    await expect(decryptVendorSessionInfo(Buffer.alloc(16))).rejects.toThrow(
      'VendorSessionInfo too short: 16'
    )
  })

  test('decryptVendorSessionText decrypts and trims the final text', async () => {
    const iv = Buffer.from('00112233445566778899aabbccddeeff', 'hex')
    const plain = Buffer.from('hello from test  \0\0', 'utf8')

    const cipher = createCipheriv('aes-128-cbc', VENDOR_SESSION_KEY, iv)
    const ciphertext = Buffer.concat([cipher.update(plain), cipher.final()])

    const decodedPayload = Buffer.concat([iv, ciphertext])
    const raw = Buffer.from(decodedPayload.toString('base64') + '\0\0', 'ascii')

    await expect(decryptVendorSessionText(raw)).resolves.toBe('hello from test')
  })
})
