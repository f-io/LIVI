import { promises as fsp } from 'fs'
import { FirmwareUpdateService } from '@main/services/projection/services/FirmwareUpdateService'

describe('FirmwareUpdateService', () => {
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
    }
  })

  test('getLocalFirmwareStatus returns not-ready when manifest missing', async () => {
    const svc = new FirmwareUpdateService() as any
    svc.readManifest = jest.fn(async () => null)

    const out = await svc.getLocalFirmwareStatus({
      boxInfo: { productType: 'A', uuid: 'u', MFD: 'm' }
    })
    expect(out).toEqual({ ok: true, ready: false, reason: 'No downloaded firmware manifest' })
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

    const statSpy = jest.spyOn(fsp, 'stat').mockResolvedValue({
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

    statSpy.mockRestore()
  })
})
