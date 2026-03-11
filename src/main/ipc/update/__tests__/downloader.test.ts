import { EventEmitter } from 'events'

jest.mock('fs', () => ({
  createWriteStream: jest.fn(),
  existsSync: jest.fn(() => false),
  promises: { unlink: jest.fn(() => Promise.resolve()) }
}))

jest.mock('node:https', () => ({
  get: jest.fn()
}))

import { createWriteStream, existsSync, promises as fsp } from 'fs'
import * as https from 'node:https'
import { downloadWithProgress } from '@main/ipc/update/downloader'

function makeReq(): EventEmitter & { destroy: jest.Mock } {
  const req = new EventEmitter() as EventEmitter & { destroy: jest.Mock }
  req.destroy = jest.fn()
  return req
}

function makeFile(): EventEmitter & { destroy: jest.Mock; close: (cb: () => void) => void } {
  const file = new EventEmitter() as EventEmitter & {
    destroy: jest.Mock
    close: (cb: () => void) => void
  }
  file.destroy = jest.fn()
  file.close = (cb) => cb()
  return file
}

describe('downloadWithProgress', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('downloads file and reports progress', async () => {
    const file = makeFile()
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = { 'content-length': '5' }
      res.pipe = () => {
        res.emit('data', Buffer.from('abc'))
        res.emit('data', Buffer.from('de'))
        file.emit('finish')
      }
      cb(res)
      return req
    })

    const progress = jest.fn()
    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', progress)

    await expect(promise).resolves.toBeUndefined()
    expect(progress).toHaveBeenCalledWith({ received: 3, total: 5, percent: 0.6 })
    expect(progress).toHaveBeenCalledWith({ received: 5, total: 5, percent: 1 })
  })

  test('rejects on non-200 response', async () => {
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
      }
      res.statusCode = 500
      res.headers = {}
      cb(res)
      return req
    })

    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})
    await expect(promise).rejects.toThrow('HTTP 500')
  })

  test('cancel aborts request and removes partial file', async () => {
    const file = makeFile()
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(existsSync as jest.Mock).mockReturnValue(true)
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = { 'content-length': '100' }
      res.pipe = () => {
        // keep request hanging to exercise cancel path
      }
      cb(res)
      return req
    })

    const { promise, cancel } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})
    cancel()

    await expect(promise).rejects.toThrow('aborted')
    expect(fsp.unlink as jest.Mock).toHaveBeenCalledWith('/tmp/file')
  })
})
