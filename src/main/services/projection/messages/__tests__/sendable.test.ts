import { MessageType } from '@main/services/projection/messages/common'
import {
  boxTmpPath,
  FileAddress,
  HeartBeat,
  LogoType,
  SendAndroidAutoDpi,
  SendAudio,
  SendAutoConnectByBtAddress,
  SendBluetoothPairedList,
  SendBoolean,
  SendBoxSettings,
  SendCloseDongle,
  SendClusterFocusRelease,
  SendClusterFocusRequest,
  SendCommand,
  SendDisconnectPhone,
  SendFile,
  SendForgetBluetoothAddr,
  SendGnssData,
  SendIconConfig,
  SendLiviWeb,
  SendLogoType,
  SendMultiTouch,
  SendNumber,
  SendOpen,
  SendSafeArea,
  SendServerCgiScript,
  SendString,
  SendTmpFile,
  SendTouch,
  SendViewArea
} from '@main/services/projection/messages/sendable'

describe('sendable messages', () => {
  test('SendCommand serialises message header + mapped payload', async () => {
    const msg = new SendCommand('frame')
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.Command)
    expect(buf.readUInt32LE(16)).toBeGreaterThanOrEqual(0)
  })

  test('SendBluetoothPairedList appends NUL terminator', async () => {
    const msg = new SendBluetoothPairedList('Device A')
    const payload = msg.getPayload()
    expect(payload[payload.length - 1]).toBe(0)
  })

  test('SendBluetoothPairedList does not duplicate trailing NUL', async () => {
    const msg = new SendBluetoothPairedList('Device A\0')
    const payload = msg.getPayload()
    expect(payload.toString('utf8')).toBe('Device A\0')
  })

  test('SendGnssData normalizes line endings and appends CRLF', async () => {
    const msg = new SendGnssData('$GPGGA,1\n$GPRMC,2')
    expect(msg.getPayload().toString('ascii')).toBe('$GPGGA,1\r\n$GPRMC,2\r\n')
  })

  test('SendGnssData returns empty payload for empty input', async () => {
    const msg = new SendGnssData('')
    expect(msg.getPayload().toString('ascii')).toBe('')
  })

  test('SendTouch clamps coordinates into 0..10000 space', async () => {
    const msg = new SendTouch(-1, 2, 1 as any)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(1)
    expect(payload.readUInt32LE(4)).toBe(0)
    expect(payload.readUInt32LE(8)).toBe(10000)
  })

  test('SendMultiTouch concatenates touch payloads', async () => {
    const msg = new SendMultiTouch([
      { id: 1, x: 0.1, y: 0.2, action: 2 },
      { id: 2, x: 0.3, y: 0.4, action: 3 }
    ] as any)

    const payload = msg.getPayload()
    expect(payload.length).toBe(32)
  })

  test('SendAudio serializes decodeType and pcm payload', async () => {
    const pcm = new Int16Array([100, -200])
    const msg = new SendAudio(pcm, 7)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(7)
    expect(payload.readUInt32LE(8)).toBe(3)
    expect(payload.subarray(12).length).toBe(pcm.byteLength)
  })

  test('SendFile encodes file name and content lengths', async () => {
    const msg = new SendFile(Buffer.from([1, 2, 3]), '/tmp/test.bin')
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')
    const contentLen = payload.readUInt32LE(4 + nameLen)

    expect(name).toBe('/tmp/test.bin')
    expect(contentLen).toBe(3)
  })

  test('boxTmpPath sanitizes path and defaults empty names', async () => {
    expect(boxTmpPath('a/b/c.img')).toBe('/tmp/c.img')
    expect(boxTmpPath('   ')).toBe('/tmp/update.img')
  })

  test('SendTmpFile always targets /tmp/<file>', async () => {
    const msg = new SendTmpFile(Buffer.from([1, 2, 3]), '/weird/path/fw.img')
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')

    expect(name).toBe('/tmp/fw.img')
  })

  test('SendViewArea writes 24-byte screen and origin payload', async () => {
    const msg = new SendViewArea(800, 480)
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const bodyOffset = 4 + nameLen + 4
    const body = payload.subarray(bodyOffset)

    expect(body.length).toBe(24)
    expect(body.readUInt32LE(0)).toBe(800)
    expect(body.readUInt32LE(4)).toBe(480)
    expect(body.readUInt32LE(8)).toBe(800)
    expect(body.readUInt32LE(12)).toBe(480)
    expect(body.readUInt32LE(16)).toBe(0)
    expect(body.readUInt32LE(20)).toBe(0)
  })

  test('SendViewArea insets shrink the view rect and shift the origin', async () => {
    const msg = new SendViewArea(800, 480, {
      insets: { top: 10, bottom: 20, left: 30, right: 40 }
    })
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const body = payload.subarray(4 + nameLen + 4)

    expect(body.readUInt32LE(0)).toBe(800)
    expect(body.readUInt32LE(4)).toBe(480)
    expect(body.readUInt32LE(8)).toBe(730)
    expect(body.readUInt32LE(12)).toBe(450)
    expect(body.readUInt32LE(16)).toBe(30)
    expect(body.readUInt32LE(20)).toBe(10)
  })

  test('SendViewArea rounds odd insets and dimensions down to a multiple of 2', async () => {
    const msg = new SendViewArea(1281, 801, {
      insets: { top: 11, bottom: 13, left: 15, right: 17 }
    })
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const body = payload.subarray(4 + nameLen + 4)

    expect(body.readUInt32LE(0)).toBe(1280)
    expect(body.readUInt32LE(4)).toBe(800)
    expect(body.readUInt32LE(8) % 2).toBe(0)
    expect(body.readUInt32LE(12) % 2).toBe(0)
    expect(body.readUInt32LE(16)).toBe(14)
    expect(body.readUInt32LE(20)).toBe(10)
  })

  test('SendSafeArea rounds odd insets and dimensions down to a multiple of 2', async () => {
    const msg = new SendSafeArea(1281, 801, {
      insets: { top: 11, bottom: 13, left: 15, right: 17 }
    })
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const body = payload.subarray(4 + nameLen + 4)

    expect(body.readUInt32LE(0) % 2).toBe(0)
    expect(body.readUInt32LE(4) % 2).toBe(0)
    expect(body.readUInt32LE(8)).toBe(14)
    expect(body.readUInt32LE(12)).toBe(10)
  })

  test('SendViewArea and SendSafeArea target the cluster files via address option', async () => {
    const view = new SendViewArea(800, 480, {
      address: FileAddress.HU_NAVISCREEN_VIEWAREA_INFO
    })
    const safe = new SendSafeArea(800, 480, {
      address: FileAddress.HU_NAVISCREEN_SAFEAREA_INFO
    })

    const viewName = view
      .getPayload()
      .subarray(4, 4 + view.getPayload().readUInt32LE(0))
      .toString('ascii')
      .replace(/\0+$/g, '')
    const safeName = safe
      .getPayload()
      .subarray(4, 4 + safe.getPayload().readUInt32LE(0))
      .toString('ascii')
      .replace(/\0+$/g, '')

    expect(viewName).toBe('/etc/RiddleBoxData/HU_NAVISCREEN_VIEWAREA_INFO')
    expect(safeName).toBe('/etc/RiddleBoxData/HU_NAVISCREEN_SAFEAREA_INFO')
  })

  test('SendSafeArea computes safe area and drawOutside flag', async () => {
    const msg = new SendSafeArea(1000, 500, {
      insets: { top: 10, bottom: 20, left: 30, right: 40 }
    })
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const bodyOffset = 4 + nameLen + 4
    const body = payload.subarray(bodyOffset)

    expect(body.readUInt32LE(0)).toBe(930)
    expect(body.readUInt32LE(4)).toBe(470)
    expect(body.readUInt32LE(8)).toBe(30)
    expect(body.readUInt32LE(12)).toBe(10)
    expect(body.readUInt32LE(16)).toBe(1)
  })

  test('SendNumber and SendBoolean encode uint32 payloads', async () => {
    const num = new SendNumber(42, FileAddress.DPI)
    const boolTrue = new SendBoolean(true, FileAddress.NIGHT_MODE)
    const boolFalse = new SendBoolean(false, FileAddress.NIGHT_MODE)

    const numPayload = num.getPayload()
    const truePayload = boolTrue.getPayload()
    const falsePayload = boolFalse.getPayload()

    const numNameLen = numPayload.readUInt32LE(0)
    const numBody = numPayload.subarray(4 + numNameLen + 4)

    const trueNameLen = truePayload.readUInt32LE(0)
    const trueBody = truePayload.subarray(4 + trueNameLen + 4)

    const falseNameLen = falsePayload.readUInt32LE(0)
    const falseBody = falsePayload.subarray(4 + falseNameLen + 4)

    expect(numBody.readUInt32LE(0)).toBe(42)
    expect(trueBody.readUInt32LE(0)).toBe(1)
    expect(falseBody.readUInt32LE(0)).toBe(0)
  })

  test('SendString strips non-ascii, removes line breaks and truncates to 16 chars', async () => {
    const msg = new SendString('ÄBC\nDEF\rGHIJKLMNOPQRST', FileAddress.BOX_NAME)
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen)

    expect(body.toString('ascii')).toBe('A?BC?DEF?GHIJKLM')
  })

  test('SendOpen writes 28-byte payload with dimensions fps and phone mode', async () => {
    const msg = new SendOpen({ width: 800, height: 480, fps: 60 }, 3 as any)
    const payload = msg.getPayload()

    expect(payload.length).toBe(28)
    expect(payload.readUInt32LE(0)).toBe(800)
    expect(payload.readUInt32LE(4)).toBe(480)
    expect(payload.readUInt32LE(8)).toBe(60)
    expect(payload.readUInt32LE(24)).toBe(3)
  })

  test('SendSafeArea respects explicit drawOutside=false even when insets exist', async () => {
    const msg = new SendSafeArea(1000, 500, {
      insets: { top: 10, bottom: 20, left: 30, right: 40 },
      drawOutside: false
    })
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const bodyOffset = 4 + nameLen + 4
    const body = payload.subarray(bodyOffset)

    expect(body.readUInt32LE(0)).toBe(930)
    expect(body.readUInt32LE(4)).toBe(470)
    expect(body.readUInt32LE(16)).toBe(0)
  })

  test('SendAndroidAutoDpi writes a positive dpi number into DPI file', async () => {
    const msg = new SendAndroidAutoDpi(1280, 720)
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')
    const body = payload.subarray(4 + nameLen + 4)

    expect(name).toBe(FileAddress.DPI)
    expect(body.readUInt32LE(0)).toBeGreaterThan(0)
  })

  test('SendLogoType writes logo type as uint32 payload', async () => {
    const msg = new SendLogoType(LogoType.Siri)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(LogoType.Siri)
  })

  test('HeartBeat serialises header-only message', async () => {
    const msg = new HeartBeat()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.HeartBeat)
  })

  test('SendCloseDongle serialises header-only message', async () => {
    const msg = new SendCloseDongle()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.CloseDongle)
  })

  test('SendDisconnectPhone serialises header-only message', async () => {
    const msg = new SendDisconnectPhone()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.DisconnectPhone)
  })

  test('SendClusterFocusRequest serialises header-only message', async () => {
    const msg = new SendClusterFocusRequest()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.ClusterFocusRequest)
  })

  test('SendClusterFocusRelease serialises header-only message', async () => {
    const msg = new SendClusterFocusRelease()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.ClusterFocusRelease)
  })

  test('SendIconConfig includes oemIconLabel when oemName is provided', async () => {
    const msg = new SendIconConfig({ oemName: 'My Car' })
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).toContain(`oemIconPath = ${FileAddress.OEM_ICON}`)
    expect(body).toContain('oemIconLabel = My Car')
  })

  test('SendIconConfig omits oemIconLabel when oemName is blank', async () => {
    const msg = new SendIconConfig({ oemName: '   ' })
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).not.toContain('oemIconLabel =')
  })

  test('SendBoxSettings builds expected dashboard, gnss and fallback wifi fields', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 300,
        wifiChannel: Number.NaN,
        wifiType: '5ghz',
        samplingFrequency: 1,
        callQuality: 2,
        gps: true,
        autoConn: true,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 1,
        micType: 1,
        disableAudioOutput: true,
        dashboardMediaInfo: true,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: true,
        gnssGps: true,
        gnssGlonass: false,
        gnssGalileo: true,
        gnssBeiDou: false
      } as any,
      123456
    )

    const payload = msg.getPayload()
    const body = JSON.parse(payload.toString('ascii'))

    expect(body.mediaDelay).toBe(300)
    expect(body.syncTime).toBe(123456)
    expect(body.wifiChannel).toBe(36)
    expect(body.gps).toBe(1)
    expect(body.autoConn).toBe(1)
    expect(body.UseBTPhone).toBe(0)
    expect(body.DashboardInfo).toBe(5)
    expect(body.GNSSCapability).toBe(5)
    expect(body.wifiName).toBe('CarName (D)')
    expect(body.btName).toBe('CarName (D)')
    expect(body.boxName).toBe('OEM')
    expect(body.OemName).toBe('OEM')
  })

  test('SendBoxSettings registers a full-size cluster naviScreenInfo.safearea', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 6,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: true,
        autoConn: false,
        UseBTPhone: true,
        carName: 'CarName',
        oemName: '',
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        dashboards: { dash3: { main: true, dash: false, aux: false } },
        clusterWidth: 800,
        clusterHeight: 480,
        clusterFps: 30,
        clusterSafeAreaTop: 10,
        clusterSafeAreaBottom: 20,
        clusterSafeAreaLeft: 30,
        clusterSafeAreaRight: 40
      } as any,
      1
    )

    const payload = msg.getPayload()
    const body = JSON.parse(payload.toString('ascii'))

    expect(body.naviScreenInfo).toEqual({
      width: 800,
      height: 480,
      fps: 30,
      safearea: {
        width: 800,
        height: 480,
        x: 0,
        y: 0,
        outside: 0
      }
    })
  })

  test('SendBoxSettings hardcodes cluster safearea.outside to 0 on dongle path', async () => {
    // outside stays 0, non-zero values break the dongle firmware.
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: '',
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        dashboards: { dash3: { main: true, dash: false, aux: false } },
        clusterWidth: 800,
        clusterHeight: 480,
        clusterFps: 24,
        clusterViewAreaTop: 10,
        clusterViewAreaBottom: 0,
        clusterViewAreaLeft: 0,
        clusterViewAreaRight: 0
      } as any,
      1
    )

    const payload = msg.getPayload()
    const body = JSON.parse(payload.toString('ascii'))

    expect(body.naviScreenInfo.safearea.outside).toBe(0)
  })

  test('SendServerCgiScript targets LIVI_CGI and contains non-empty script', async () => {
    const msg = new SendServerCgiScript()
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen)

    expect(name).toBe(FileAddress.LIVI_CGI)
    expect(body.byteLength).toBeGreaterThan(0)
  })

  test('SendLiviWeb targets LIVI_WEB and contains non-empty html payload', async () => {
    const msg = new SendLiviWeb()
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const name = payload
      .subarray(4, 4 + nameLen)
      .toString('ascii')
      .replace(/\0+$/g, '')
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen)

    expect(name).toBe(FileAddress.LIVI_WEB)
    expect(body.byteLength).toBeGreaterThan(0)
  })

  test('SendAutoConnectByBtAddress stores ascii bluetooth address payload', async () => {
    const msg = new SendAutoConnectByBtAddress('AA:BB:CC:DD:EE:FF')

    expect(msg.type).toBe(MessageType.WifiStatusData)
    expect(msg.getPayload().toString('ascii')).toBe('AA:BB:CC:DD:EE:FF')
  })

  test('SendForgetBluetoothAddr stores ascii bluetooth address payload', async () => {
    const msg = new SendForgetBluetoothAddr('11:22:33:44:55:66')

    expect(msg.type).toBe(MessageType.ForgetBluetoothAddr)
    expect(msg.getPayload().toString('ascii')).toBe('11:22:33:44:55:66')
  })

  test('SendBoxSettings logs payload when DEBUG is true', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(function () {})

    vi.resetModules()

    await vi.isolateModules(async () => {
      vi.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      const { SendBoxSettings } = await import('@main/services/projection/messages/sendable')

      const msg = new SendBoxSettings(
        {
          width: 1280,
          height: 720,
          fps: 60,
          mediaDelay: 0,
          wifiChannel: 1,
          wifiType: '2.4ghz',
          samplingFrequency: 1,
          callQuality: 1,
          gps: false,
          autoConn: false,
          UseBTPhone: false,
          carName: 'CarName',
          oemName: 'OEM',
          hand: 0,
          micType: 0,
          disableAudioOutput: false,
          dashboardMediaInfo: false,
          dashboardVehicleInfo: false,
          dashboardRouteInfo: false,
          gnssGps: false,
          gnssGlonass: false,
          gnssGalileo: false,
          gnssBeiDou: false
        },
        123
      )

      const payload = msg.getPayload()
      const body = JSON.parse(payload.toString('ascii'))

      expect(body.syncTime).toBe(123)
    })

    expect(logSpy).toHaveBeenCalledWith('[SendBoxSettings]', expect.any(String))

    logSpy.mockRestore()
    vi.resetModules()
    vi.doUnmock('@main/constants')
  })

  test('SendBoxSettings logs payload when DEBUG is true', async () => {
    const logSpy = vi.spyOn(console, 'log').mockImplementation(function () {})

    vi.resetModules()

    await vi.isolateModules(async () => {
      vi.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      const { SendBoxSettings } = await import('@main/services/projection/messages/sendable')

      const msg = new SendBoxSettings(
        {
          width: 1280,
          height: 720,
          fps: 60,
          mediaDelay: 0,
          wifiChannel: 1,
          wifiType: '2.4ghz',
          samplingFrequency: 1,
          callQuality: 1,
          gps: false,
          autoConn: false,
          UseBTPhone: false,
          carName: 'CarName',
          oemName: 'OEM',
          hand: 0,
          micType: 0,
          disableAudioOutput: false,
          dashboardMediaInfo: false,
          dashboardVehicleInfo: false,
          dashboardRouteInfo: false,
          gnssGps: false,
          gnssGlonass: false,
          gnssGalileo: false,
          gnssBeiDou: false
        },
        123
      )

      const payload = msg.getPayload()
      const body = JSON.parse(payload.toString('ascii'))

      expect(body.syncTime).toBe(123)
    })

    expect(logSpy).toHaveBeenCalledWith('[SendBoxSettings]', expect.any(String))

    logSpy.mockRestore()
    vi.resetModules()
    vi.doUnmock('@main/constants')
  })

  test('SendBoxSettings uses current time when syncTime is null', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false
      } as any,
      null
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))
    expect(typeof body.syncTime).toBe('number')
    expect(body.syncTime).toBeGreaterThan(0)
  })

  test('SendBoxSettings falls back to carName when oemName is undefined', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: undefined,
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: true
      } as any,
      1
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))

    expect(body.boxName).toBe('CarName')
    expect(body.OemName).toBe('CarName')
    expect(body.GNSSCapability).toBe(8)
  })

  test('SendBoxSettings uses default navi safe-area zeros when values are undefined', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        dashboards: { dash3: { main: true, dash: false, aux: false } },
        clusterWidth: 800,
        clusterHeight: 480,
        clusterFps: 30,
        clusterSafeAreaTop: undefined,
        clusterSafeAreaBottom: undefined,
        clusterSafeAreaLeft: undefined,
        clusterSafeAreaRight: undefined
      } as any,
      1
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))

    expect(body.naviScreenInfo).toEqual({
      width: 800,
      height: 480,
      fps: 30,
      safearea: {
        width: 800,
        height: 480,
        x: 0,
        y: 0,
        outside: 0
      }
    })
  })

  test('SendBoxSettings constructor uses default syncTime parameter when omitted', async () => {
    const msg = new SendBoxSettings({
      width: 1280,
      height: 720,
      fps: 60,
      mediaDelay: 0,
      wifiChannel: 1,
      wifiType: '2.4ghz',
      samplingFrequency: 1,
      callQuality: 1,
      gps: false,
      autoConn: false,
      UseBTPhone: false,
      carName: 'CarName',
      oemName: 'OEM',
      hand: 0,
      micType: 0,
      disableAudioOutput: false,
      dashboardMediaInfo: false,
      dashboardVehicleInfo: false,
      dashboardRouteInfo: false,
      gnssGps: false,
      gnssGlonass: false,
      gnssGalileo: false,
      gnssBeiDou: false
    } as any)

    const body = JSON.parse(msg.getPayload().toString('ascii'))
    expect(typeof body.syncTime).toBe('number')
    expect(body.syncTime).toBeGreaterThan(0)
  })

  test('SendIconConfig handles undefined oemName without label', async () => {
    const msg = new SendIconConfig({})
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).not.toContain('oemIconLabel =')
  })

  test('SendGnssData treats nullish input as empty string', async () => {
    const msg = new SendGnssData(undefined as any)
    expect(msg.getPayload().toString('ascii')).toBe('')
  })

  test('SendSafeArea uses default options and zero insets when omitted', async () => {
    const msg = new SendSafeArea(1000, 500)
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const bodyOffset = 4 + nameLen + 4
    const body = payload.subarray(bodyOffset)

    expect(body.readUInt32LE(0)).toBe(1000)
    expect(body.readUInt32LE(4)).toBe(500)
    expect(body.readUInt32LE(8)).toBe(0)
    expect(body.readUInt32LE(12)).toBe(0)
    expect(body.readUInt32LE(16)).toBe(0)
  })

  test('boxTmpPath falls back correctly for empty filename', async () => {
    expect(boxTmpPath('')).toBe('/tmp/update.img')
  })

  test('SendBoxSettings uses 2.4ghz fallback channel and sets vehicle/glonass flags', async () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: Number.NaN,
        wifiType: '2.4ghz',
        samplingFrequency: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        disableAudioOutput: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: true,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: true,
        gnssGalileo: false,
        gnssBeiDou: false
      } as any,
      1
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))

    expect(body.wifiChannel).toBe(1)
    expect(body.DashboardInfo).toBe(2)
    expect(body.GNSSCapability).toBe(2)
  })
})
