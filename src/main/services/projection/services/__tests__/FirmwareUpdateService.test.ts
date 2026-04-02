import { SendTmpFile } from '@main/services/projection/messages/sendable'
import { FirmwareUpdateService } from '@main/services/projection/services/FirmwareUpdateService'
import { app } from 'electron'
import { existsSync, promises as fsp } from 'fs'

jest.mock('electron', () => ({
  app: {
    getPath: jest.fn(() => '/tmp/app-user-data')
  },
  net: {
    request: jest.fn()
  }
}))

jest.mock('fs', () => ({
  existsSync: jest.fn(() => false),
  createWriteStream: jest.fn(),
  promises: {
    mkdir: jest.fn(),
    unlink: jest.fn(),
    rename: jest.fn(),
    writeFile: jest.fn(),
    readFile: jest.fn(),
    stat: jest.fn()
  }
}))

describe('FirmwareUpdateService', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    ;(existsSync as jest.Mock).mockReturnValue(false)
  })

  afterEach(() => {
    jest.restoreAllMocks()
  })

  test('checkForUpdate validates required fields before network call', async () => {
    const svc = new FirmwareUpdateService()

    await expect(svc.checkForUpdate({ appVer: '' } as any)).resolves.toEqual({
      ok: false,
      error: 'Missing appVer'
    })

    await expect(
      svc.checkForUpdate({ appVer: '1.0.0', dongleFwVersion: null } as any)
    ).resolves.toEqual({
      ok: false,
      error: 'Missing dongleFwVersion (ver)'
    })
  })

  test('checkForUpdate validates missing box fields', async () => {
    const svc = new FirmwareUpdateService()

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: {}
      } as any)
    ).resolves.toEqual({
      ok: false,
      error: 'Missing boxInfo.uuid'
    })

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: { uuid: 'u' }
      } as any)
    ).resolves.toEqual({
      ok: false,
      error: 'Missing boxInfo.MFD'
    })

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: { uuid: 'u', MFD: 'm' }
      } as any)
    ).resolves.toEqual({
      ok: false,
      error: 'Missing boxInfo.productType (model)'
    })
  })

  test('checkForUpdate parses API payload and computes hasUpdate', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () =>
      JSON.stringify({
        err: 0,
        ver: '2.0.0',
        notes: 'note',
        size: 111,
        id: 'id1',
        token: 'tok'
      })
    )

    const result = await svc.checkForUpdate({
      appVer: '1.0.0',
      dongleFwVersion: '1.0.0',
      boxInfo: { uuid: 'u', MFD: 'm', productType: 'A15W' }
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.hasUpdate).toBe(true)
      expect(result.latestVer).toBe('2.0.0')
      expect(result.token).toBe('tok')
      expect(result.request?.fwn).toBe('A15W_Update.img')
      expect(result.request).toEqual({
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: 'A15W_Update.img',
        model: 'A15W'
      })
    }
  })

  test('checkForUpdate accepts stringified boxInfo json', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () => JSON.stringify({ err: 0, ver: '2.0.0' }))

    const result = await svc.checkForUpdate({
      appVer: '1.0.0',
      dongleFwVersion: '1.0.0',
      boxInfo: JSON.stringify({ uuid: 'u', MFD: 'm', productType: 'A15W' })
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.request?.uuid).toBe('u')
      expect(result.request?.mfd).toBe('m')
      expect(result.request?.model).toBe('A15W')
    }
  })

  test('checkForUpdate returns no update when versions match', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () =>
      JSON.stringify({
        err: 0,
        ver: '1.0.0',
        notes: 'same',
        size: 111
      })
    )

    const result = await svc.checkForUpdate({
      appVer: '1.0.0',
      dongleFwVersion: '1.0.0',
      boxInfo: { uuid: 'u', MFD: 'm', productType: 'A15W' }
    })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.hasUpdate).toBe(false)
      expect(result.latestVer).toBe('1.0.0')
    }
  })

  test('checkForUpdate returns error when API err is non-zero', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () => JSON.stringify({ err: 7 }))

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: { uuid: 'u', MFD: 'm', productType: 'A15W' }
      })
    ).resolves.toEqual({
      ok: false,
      error: 'checkBox err=7'
    })
  })

  test('checkForUpdate handles invalid json response', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () => 'not-json')

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: { uuid: 'u', MFD: 'm', productType: 'A15W' }
      })
    ).resolves.toEqual({
      ok: false,
      error: 'checkBox err=-1'
    })
  })

  test('checkForUpdate returns network error message when request throws', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.httpPostForm = jest.fn(async () => {
      throw new Error('network down')
    })

    await expect(
      svc.checkForUpdate({
        appVer: '1.0.0',
        dongleFwVersion: '1.0.0',
        boxInfo: { uuid: 'u', MFD: 'm', productType: 'A15W' }
      })
    ).resolves.toEqual({
      ok: false,
      error: 'network down'
    })
  })

  test('downloadFirmwareToHost returns error when token is missing', async () => {
    const svc = new FirmwareUpdateService()

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      request: {
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: 'A15W_Update.img',
        model: 'A15W'
      },
      raw: {}
    })

    expect(out).toEqual({
      ok: false,
      error: 'Missing token'
    })
  })

  test('downloadFirmwareToHost returns error when request payload is missing', async () => {
    const svc = new FirmwareUpdateService()

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      token: 'tok',
      raw: {}
    } as any)

    expect(out).toEqual({
      ok: false,
      error: 'Missing request payload from checkForUpdate()'
    })
  })

  test('downloadFirmwareToHost returns error when destination file exists and overwrite is false', async () => {
    const svc = new FirmwareUpdateService()
    ;(existsSync as jest.Mock).mockReturnValue(true)

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      token: 'tok',
      request: {
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: 'A15W_Update.img',
        model: 'A15W'
      },
      raw: {}
    })

    expect(out).toEqual({
      ok: false,
      error: 'File already exists: /tmp/app-user-data/firmware/A15W_Update.img'
    })
  })

  test('downloadFirmwareToHost removes existing destination when overwrite is true', async () => {
    const svc = new FirmwareUpdateService() as any
    ;(existsSync as jest.Mock).mockReturnValue(true)
    svc.downloadToFile = jest.fn(async () => ({ bytes: 10 }))
    svc.writeManifest = jest.fn(async () => undefined)

    const out = await svc.downloadFirmwareToHost(
      {
        ok: true,
        hasUpdate: true,
        latestVer: '2.0.0',
        size: 10,
        id: 'id1',
        token: 'tok',
        request: {
          lang: 0,
          code: 37,
          appVer: '1.0.0',
          ver: '1.0.0',
          uuid: 'u',
          mfd: 'm',
          fwn: 'A15W_Update.img',
          model: 'A15W'
        },
        raw: {}
      },
      { overwrite: true }
    )

    expect(fsp.unlink).toHaveBeenCalledWith('/tmp/app-user-data/firmware/A15W_Update.img')
    expect(fsp.rename).toHaveBeenCalledWith(
      '/tmp/app-user-data/firmware/A15W_Update.img.part',
      '/tmp/app-user-data/firmware/A15W_Update.img'
    )
    expect(out).toEqual({
      ok: true,
      path: '/tmp/app-user-data/firmware/A15W_Update.img',
      bytes: 10
    })
  })

  test('downloadFirmwareToHost returns mismatch error and deletes tmp file when size differs', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.downloadToFile = jest.fn(async () => ({ bytes: 9 }))
    svc.writeManifest = jest.fn(async () => undefined)

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 10,
      token: 'tok',
      request: {
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: 'A15W_Update.img',
        model: 'A15W'
      },
      raw: {}
    })

    expect(fsp.unlink).toHaveBeenCalledWith('/tmp/app-user-data/firmware/A15W_Update.img.part')
    expect(out).toEqual({
      ok: false,
      error: 'Downloaded size mismatch (9 != 10)'
    })
    expect(fsp.rename).not.toHaveBeenCalled()
  })

  test('downloadFirmwareToHost succeeds and writes manifest', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.downloadToFile = jest.fn(async () => ({ bytes: 10 }))
    svc.writeManifest = jest.fn(async () => undefined)

    const onProgress = jest.fn()

    const out = await svc.downloadFirmwareToHost(
      {
        ok: true,
        hasUpdate: true,
        latestVer: '2.0.0',
        size: 10,
        id: 'id1',
        token: 'tok',
        request: {
          lang: 0,
          code: 37,
          appVer: '1.0.0',
          ver: '1.0.0',
          uuid: 'u',
          mfd: 'm',
          fwn: 'A15W_Update.img',
          model: 'A15W'
        },
        raw: {}
      },
      { onProgress }
    )

    expect(fsp.mkdir).toHaveBeenCalledWith('/tmp/app-user-data/firmware', { recursive: true })
    expect(svc.downloadToFile).toHaveBeenCalledWith({
      url: 'http://api.paplink.cn/a/upgrade/down',
      body: expect.any(String),
      token: 'tok',
      tmpPath: '/tmp/app-user-data/firmware/A15W_Update.img.part',
      onProgress
    })
    expect(fsp.rename).toHaveBeenCalledWith(
      '/tmp/app-user-data/firmware/A15W_Update.img.part',
      '/tmp/app-user-data/firmware/A15W_Update.img'
    )
    expect(svc.writeManifest).toHaveBeenCalledWith({
      createdAt: expect.any(String),
      path: '/tmp/app-user-data/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    })
    expect(out).toEqual({
      ok: true,
      path: '/tmp/app-user-data/firmware/A15W_Update.img',
      bytes: 10
    })
  })

  test('downloadFirmwareToHost uses fallback file name when req.fwn is empty', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.downloadToFile = jest.fn(async () => ({ bytes: 10 }))
    svc.writeManifest = jest.fn(async () => undefined)

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      latestVer: '2.0.0',
      size: 10,
      token: 'tok',
      request: {
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: '   ',
        model: 'A15W'
      },
      raw: {}
    })

    expect(out).toEqual({
      ok: true,
      path: '/tmp/app-user-data/firmware/Auto_Box_Update.img',
      bytes: 10
    })
  })

  test('downloadFirmwareToHost returns thrown error message', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.downloadToFile = jest.fn(async () => {
      throw new Error('download broken')
    })

    const out = await svc.downloadFirmwareToHost({
      ok: true,
      hasUpdate: true,
      token: 'tok',
      request: {
        lang: 0,
        code: 37,
        appVer: '1.0.0',
        ver: '1.0.0',
        uuid: 'u',
        mfd: 'm',
        fwn: 'A15W_Update.img',
        model: 'A15W'
      },
      raw: {}
    })

    expect(out).toEqual({
      ok: false,
      error: 'download broken'
    })
  })

  test('getLocalFirmwareStatus returns not-ready when manifest missing', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => null)

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A', uuid: 'u', MFD: 'm' }
    })
    expect(out).toEqual({ ok: true, ready: false, reason: 'No downloaded firmware manifest' })
  })

  test('getLocalFirmwareStatus validates missing boxInfo fields', async () => {
    const svc = new FirmwareUpdateService()

    await expect(svc.getLocalFirmwareStatus({ boxInfo: {} } as any)).resolves.toEqual({
      ok: true,
      ready: false,
      reason: 'Missing boxInfo.productType'
    })

    await expect(
      svc.getLocalFirmwareStatus({ boxInfo: { productType: 'A15W' } } as any)
    ).resolves.toEqual({
      ok: true,
      ready: false,
      reason: 'Missing boxInfo.uuid'
    })

    await expect(
      svc.getLocalFirmwareStatus({ boxInfo: { productType: 'A15W', uuid: 'u' } } as any)
    ).resolves.toEqual({
      ok: true,
      ready: false,
      reason: 'Missing boxInfo.MFD'
    })
  })

  test('getLocalFirmwareStatus detects model mismatch', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/B15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'B15W',
      uuid: 'u',
      mfd: 'm'
    }))

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Model mismatch (have A15W, expected B15W)'
    })
  })

  test('getLocalFirmwareStatus detects uuid mismatch', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'other',
      mfd: 'm'
    }))

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Dongle UUID mismatch (dongle was swapped)'
    })
  })

  test('getLocalFirmwareStatus detects mfd mismatch', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'other'
    }))

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Dongle MFD mismatch (dongle was swapped)'
    })
  })

  test('getLocalFirmwareStatus detects file name mismatch', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/WRONG.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    }))

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'File name mismatch (WRONG.img != A15W_Update.img)'
    })
  })

  test('getLocalFirmwareStatus returns not-ready when firmware file is missing', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    }))

    jest.spyOn(fsp, 'stat').mockRejectedValue(new Error('missing'))

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Firmware file missing'
    })
  })

  test('getLocalFirmwareStatus returns not-ready when stat is not a file', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    }))

    jest.spyOn(fsp, 'stat').mockResolvedValue({
      isFile: () => false,
      size: 10
    } as any)

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Firmware file missing'
    })
  })

  test('getLocalFirmwareStatus returns not-ready when size mismatches', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 11,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    }))

    jest.spyOn(fsp, 'stat').mockResolvedValue({
      isFile: () => true,
      size: 10
    } as any)

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: false,
      reason: 'Size mismatch (10 != 11)'
    })
  })

  test('getLocalFirmwareStatus returns ready when manifest and file match', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => ({
      path: '/tmp/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    }))

    jest.spyOn(fsp, 'stat').mockResolvedValue({
      isFile: () => true,
      size: 10
    } as any)

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A15W', uuid: 'u', MFD: 'm' }
    })

    expect(out).toEqual({
      ok: true,
      ready: true,
      path: '/tmp/firmware/A15W_Update.img',
      bytes: 10,
      model: 'A15W',
      latestVer: '2.0.0'
    })
  })

  test('startUpdate returns reason when local firmware is not ready', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: false,
      reason: 'No downloaded firmware manifest'
    }))

    const driver = { send: jest.fn() }

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any)

    expect(out).toEqual({
      ok: false,
      error: 'No downloaded firmware manifest'
    })
    expect(driver.send).not.toHaveBeenCalled()
  })

  test('startUpdate returns status error when getLocalFirmwareStatus fails', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: false,
      error: 'status broken'
    }))

    const driver = { send: jest.fn() }

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any)

    expect(out).toEqual({
      ok: false,
      error: 'status broken'
    })
  })

  test('startUpdate rejects firmware files above maxBytes', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/firmware/A15W_Update.img',
      bytes: 200,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    const driver = { send: jest.fn() }

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any, { maxBytes: 100 })

    expect(out).toEqual({
      ok: false,
      error: 'Firmware file too large (200 > 100)'
    })
    expect(driver.send).not.toHaveBeenCalled()
  })

  test('startUpdate returns error when driver send fails', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/firmware/A15W_Update.img',
      bytes: 4,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    jest.spyOn(fsp, 'readFile').mockResolvedValue(Buffer.from([1, 2, 3, 4]))
    const driver = { send: jest.fn(async () => false) }
    const onProgress = jest.fn()

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any, { onProgress })

    expect(out).toEqual({
      ok: false,
      error: 'Failed to send firmware image to dongle'
    })
    expect(driver.send).toHaveBeenCalledWith(expect.any(SendTmpFile))
    expect(onProgress).toHaveBeenCalledWith({ sent: 4, total: 4, percent: 1 })
  })

  test('startUpdate succeeds and reports progress', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/firmware/A15W_Update.img',
      bytes: 4,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    jest.spyOn(fsp, 'readFile').mockResolvedValue(Buffer.from([1, 2, 3, 4]))
    const driver = { send: jest.fn(async () => true) }
    const onProgress = jest.fn()

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any, { onProgress })

    expect(out).toEqual({ ok: true })
    expect(driver.send).toHaveBeenCalledTimes(1)
    expect(driver.send).toHaveBeenCalledWith(expect.any(SendTmpFile))
    expect(onProgress).toHaveBeenCalledWith({ sent: 4, total: 4, percent: 1 })
  })

  test('startUpdate returns thrown readFile error message', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.getLocalFirmwareStatus = jest.fn(async () => ({
      ok: true,
      ready: true,
      path: '/tmp/firmware/A15W_Update.img',
      bytes: 4,
      model: 'A15W',
      latestVer: '2.0.0'
    }))

    jest.spyOn(fsp, 'readFile').mockRejectedValue(new Error('read failed'))
    const driver = { send: jest.fn(async () => true) }

    const out = await svc.startUpdate({ appVer: '1.0.0' } as any, driver as any)

    expect(out).toEqual({
      ok: false,
      error: 'read failed'
    })
  })

  test('writeManifest writes manifest json into firmware dir', async () => {
    const svc = new FirmwareUpdateService() as any

    await svc.writeManifest({
      createdAt: '2024-01-01T00:00:00.000Z',
      path: '/tmp/app-user-data/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    })

    expect(app.getPath).toHaveBeenCalledWith('userData')
    expect(fsp.mkdir).toHaveBeenCalledWith('/tmp/app-user-data/firmware', { recursive: true })
    expect(fsp.writeFile).toHaveBeenCalledWith(
      '/tmp/app-user-data/firmware/dongle-fw.json',
      JSON.stringify(
        {
          createdAt: '2024-01-01T00:00:00.000Z',
          path: '/tmp/app-user-data/firmware/A15W_Update.img',
          expectedSize: 10,
          latestVer: '2.0.0',
          model: 'A15W',
          uuid: 'u',
          mfd: 'm'
        },
        null,
        2
      ),
      'utf8'
    )
  })

  test('readManifest returns parsed manifest', async () => {
    const svc = new FirmwareUpdateService() as any
    jest.spyOn(fsp, 'readFile').mockResolvedValue(
      JSON.stringify({
        createdAt: '2024-01-01T00:00:00.000Z',
        path: '/tmp/app-user-data/firmware/A15W_Update.img',
        expectedSize: 10,
        latestVer: '2.0.0',
        model: 'A15W',
        uuid: 'u',
        mfd: 'm'
      }) as any
    )

    const out = await svc.readManifest()

    expect(out).toEqual({
      createdAt: '2024-01-01T00:00:00.000Z',
      path: '/tmp/app-user-data/firmware/A15W_Update.img',
      expectedSize: 10,
      latestVer: '2.0.0',
      model: 'A15W',
      uuid: 'u',
      mfd: 'm'
    })
  })

  test('readManifest returns null on invalid json', async () => {
    const svc = new FirmwareUpdateService() as any
    jest.spyOn(fsp, 'readFile').mockResolvedValue('not-json' as any)

    await expect(svc.readManifest()).resolves.toBeNull()
  })

  test('readManifest returns null when parsed value is falsy or non-object', async () => {
    const svc = new FirmwareUpdateService() as any
    jest.spyOn(fsp, 'readFile').mockResolvedValue('null' as any)

    await expect(svc.readManifest()).resolves.toBeNull()
  })
})
