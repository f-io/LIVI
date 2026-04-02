import { EventEmitter } from 'events'

jest.mock('fs', () => ({
  createWriteStream: jest.fn(),
  existsSync: jest.fn(() => false),
  promises: { unlink: jest.fn(() => Promise.resolve()) }
}))

jest.mock('node:https', () => ({
  get: jest.fn()
}))

import * as https from 'node:https'
import { downloadWithProgress } from '@main/ipc/update/downloader'
import { createWriteStream, existsSync, promises as fsp } from 'fs'

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

  test('follows redirect and resolves downloaded file', async () => {
    const file = makeFile()
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(https.get as jest.Mock)
      .mockImplementationOnce((_url, cb) => {
        const req = makeReq()
        const res = new EventEmitter() as EventEmitter & {
          statusCode: number
          headers: Record<string, unknown>
        }
        res.statusCode = 302
        res.headers = { location: 'https://example.com/redirected' }
        cb(res)
        return req
      })
      .mockImplementationOnce((_url, cb) => {
        const req = makeReq()
        const res = new EventEmitter() as EventEmitter & {
          statusCode: number
          headers: Record<string, unknown>
          pipe: (dest: EventEmitter) => void
        }
        res.statusCode = 200
        res.headers = { 'content-length': '4' }
        res.pipe = () => {
          res.emit('data', Buffer.from('ab'))
          res.emit('data', Buffer.from('cd'))
          file.emit('finish')
        }
        cb(res)
        return req
      })

    const progress = jest.fn()
    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', progress)

    await expect(promise).resolves.toBeUndefined()
    expect(https.get).toHaveBeenNthCalledWith(1, 'https://example.com/a', expect.any(Function))
    expect(https.get).toHaveBeenNthCalledWith(
      2,
      'https://example.com/redirected',
      expect.any(Function)
    )
    expect(progress).toHaveBeenCalledWith({ received: 2, total: 4, percent: 0.5 })
    expect(progress).toHaveBeenCalledWith({ received: 4, total: 4, percent: 1 })
  })

  test('rejects on response stream error', async () => {
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
      res.headers = { 'content-length': '10' }
      res.pipe = () => {
        res.emit('error', new Error('response failed'))
      }
      cb(res)
      return req
    })

    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    await expect(promise).rejects.toThrow('response failed')
  })

  test('rejects on file stream error', async () => {
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
      res.headers = { 'content-length': '10' }
      res.pipe = () => {
        file.emit('error', new Error('file failed'))
      }
      cb(res)
      return req
    })

    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    await expect(promise).rejects.toThrow('file failed')
  })

  test('rejects on request error', async () => {
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = {}
      res.pipe = () => {}
      cb(res)

      process.nextTick(() => {
        req.emit('error', new Error('request failed'))
      })

      return req
    })

    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    await expect(promise).rejects.toThrow('request failed')
  })

  test('cancel is idempotent', async () => {
    const file = makeFile()
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(existsSync as jest.Mock).mockReturnValue(true)

    let reqRef: ReturnType<typeof makeReq> | undefined
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      reqRef = req
      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = { 'content-length': '100' }
      res.pipe = () => {}
      cb(res)
      return req
    })

    const { promise, cancel } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    cancel()
    cancel()

    await expect(promise).rejects.toThrow('aborted')
    expect(reqRef?.destroy).toHaveBeenCalledTimes(1)
    expect(file.destroy).toHaveBeenCalledTimes(1)
    expect(fsp.unlink as jest.Mock).toHaveBeenCalledTimes(1)
  })

  test('cancel propagates through redirect download', async () => {
    const firstReq = makeReq()
    const secondReq = makeReq()
    const redirectedFile = makeFile()

    ;(createWriteStream as jest.Mock).mockReturnValue(redirectedFile)
    ;(existsSync as jest.Mock).mockReturnValue(true)
    ;(https.get as jest.Mock)
      .mockImplementationOnce((_url, cb) => {
        const res = new EventEmitter() as EventEmitter & {
          statusCode: number
          headers: Record<string, unknown>
        }
        res.statusCode = 301
        res.headers = { location: 'https://example.com/redirected' }
        cb(res)
        return firstReq
      })
      .mockImplementationOnce((_url, cb) => {
        const res = new EventEmitter() as EventEmitter & {
          statusCode: number
          headers: Record<string, unknown>
          pipe: (dest: EventEmitter) => void
        }
        res.statusCode = 200
        res.headers = { 'content-length': '100' }
        res.pipe = () => {}
        cb(res)
        return secondReq
      })

    const { promise, cancel } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    cancel()

    await expect(promise).rejects.toThrow('aborted')
    expect(firstReq.destroy).toHaveBeenCalled()
    expect(secondReq.destroy).toHaveBeenCalled()
    expect(redirectedFile.destroy).toHaveBeenCalled()
  })

  test('ignores response, file and request events after cancel', async () => {
    const file = makeFile()
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(existsSync as jest.Mock).mockReturnValue(false)

    let reqRef!: ReturnType<typeof makeReq>
    let resRef!: EventEmitter & {
      statusCode: number
      headers: Record<string, unknown>
      pipe: (dest: EventEmitter) => void
    }
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      reqRef = req

      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = { 'content-length': '10' }
      res.pipe = () => {}
      resRef = res

      cb(res)
      return req
    })

    const progress = jest.fn()
    const { promise, cancel } = downloadWithProgress('https://example.com/a', '/tmp/file', progress)

    cancel()

    // all of these should be ignored because cancelled === true
    resRef.emit('data', Buffer.from('abc'))
    resRef.emit('error', new Error('response failed'))
    file.emit('error', new Error('file failed'))
    file.emit('finish')
    reqRef.emit('error', new Error('request failed'))

    await expect(promise).rejects.toThrow('aborted')
    expect(progress).not.toHaveBeenCalled()
  })

  test('cancel cleanup swallows destroy and unlink errors', async () => {
    const file = makeFile()
    file.destroy.mockImplementation(() => {
      throw new Error('file destroy failed')
    })
    ;(createWriteStream as jest.Mock).mockReturnValue(file)
    ;(existsSync as jest.Mock).mockReturnValue(true)
    ;(fsp.unlink as jest.Mock).mockRejectedValue(new Error('unlink failed'))

    let reqRef!: ReturnType<typeof makeReq>
    ;(https.get as jest.Mock).mockImplementation((_url, cb) => {
      const req = makeReq()
      req.destroy.mockImplementation(() => {
        throw new Error('req destroy failed')
      })
      reqRef = req

      const res = new EventEmitter() as EventEmitter & {
        statusCode: number
        headers: Record<string, unknown>
        pipe: (dest: EventEmitter) => void
      }
      res.statusCode = 200
      res.headers = { 'content-length': '100' }
      res.pipe = () => {}
      cb(res)
      return req
    })

    const { promise, cancel } = downloadWithProgress('https://example.com/a', '/tmp/file', () => {})

    expect(() => cancel()).not.toThrow()
    await expect(promise).rejects.toThrow('aborted')

    expect(reqRef.destroy).toHaveBeenCalledTimes(1)
    expect(file.destroy).toHaveBeenCalledTimes(1)
    expect(fsp.unlink as jest.Mock).toHaveBeenCalledWith('/tmp/file')
  })

  test('handles zero content-length progress as percent 0', async () => {
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
      res.headers = {}
      res.pipe = () => {
        res.emit('data', Buffer.from('abc'))
        file.emit('finish')
      }
      cb(res)
      return req
    })

    const progress = jest.fn()
    const { promise } = downloadWithProgress('https://example.com/a', '/tmp/file', progress)

    await expect(promise).resolves.toBeUndefined()
    expect(progress).toHaveBeenCalledWith({ received: 3, total: 0, percent: 0 })
  })
})
