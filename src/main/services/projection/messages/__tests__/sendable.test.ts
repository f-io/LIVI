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
  SendCommand,
  SendDisconnectPhone,
  SendFile,
  SendForgetBluetoothAddr,
  SendGnssData,
  SendIconConfig,
  SendLiviWeb,
  SendLogoType,
  SendMultiTouch,
  SendNaviFocusRelease,
  SendNaviFocusRequest,
  SendNumber,
  SendOpen,
  SendRawMessage,
  SendSafeArea,
  SendServerCgiScript,
  SendString,
  SendTmpFile,
  SendTouch,
  SendViewArea
} from '@main/services/projection/messages/sendable'

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

  test('SendBluetoothPairedList does not duplicate trailing NUL', () => {
    const msg = new SendBluetoothPairedList('Device A\0')
    const payload = msg.getPayload()
    expect(payload.toString('utf8')).toBe('Device A\0')
  })

  test('SendGnssData normalizes line endings and appends CRLF', () => {
    const msg = new SendGnssData('$GPGGA,1\n$GPRMC,2')
    expect(msg.getPayload().toString('ascii')).toBe('$GPGGA,1\r\n$GPRMC,2\r\n')
  })

  test('SendGnssData returns empty payload for empty input', () => {
    const msg = new SendGnssData('')
    expect(msg.getPayload().toString('ascii')).toBe('')
  })

  test('SendTouch clamps coordinates into 0..10000 space', () => {
    const msg = new SendTouch(-1, 2, 1 as any)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(1)
    expect(payload.readUInt32LE(4)).toBe(0)
    expect(payload.readUInt32LE(8)).toBe(10000)
  })

  test('SendMultiTouch concatenates touch payloads', () => {
    const msg = new SendMultiTouch([
      { id: 1, x: 0.1, y: 0.2, action: 2 },
      { id: 2, x: 0.3, y: 0.4, action: 3 }
    ] as any)

    const payload = msg.getPayload()
    expect(payload.length).toBe(32)
  })

  test('SendAudio serializes decodeType and pcm payload', () => {
    const pcm = new Int16Array([100, -200])
    const msg = new SendAudio(pcm, 7)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(7)
    expect(payload.readUInt32LE(8)).toBe(3)
    expect(payload.subarray(12).length).toBe(pcm.byteLength)
  })

  test('SendFile encodes file name and content lengths', () => {
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

  test('SendViewArea writes 24-byte screen and origin payload', () => {
    const msg = new SendViewArea(800, 480)
    const payload = msg.getPayload()
    const nameLen = payload.readUInt32LE(0)
    const bodyOffset = 4 + nameLen + 4
    const body = payload.subarray(bodyOffset)

    expect(body.length).toBe(24)
    expect(body.readUInt32LE(0)).toBe(800)
    expect(body.readUInt32LE(4)).toBe(480)
    expect(body.readUInt32LE(16)).toBe(0)
    expect(body.readUInt32LE(20)).toBe(0)
  })

  test('SendSafeArea computes safe area and drawOutside flag', () => {
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

  test('SendNumber and SendBoolean encode uint32 payloads', () => {
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

  test('SendString strips non-ascii, removes line breaks and truncates to 16 chars', () => {
    const msg = new SendString('ÄBC\nDEF\rGHIJKLMNOPQRST', FileAddress.BOX_NAME)
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen)

    expect(body.toString('ascii')).toBe('A?BC?DEF?GHIJKLM')
  })

  test('SendOpen writes 28-byte payload with dimensions fps and phone mode', () => {
    const msg = new SendOpen({ width: 800, height: 480, fps: 60 }, 3 as any)
    const payload = msg.getPayload()

    expect(payload.length).toBe(28)
    expect(payload.readUInt32LE(0)).toBe(800)
    expect(payload.readUInt32LE(4)).toBe(480)
    expect(payload.readUInt32LE(8)).toBe(60)
    expect(payload.readUInt32LE(24)).toBe(3)
  })

  test('SendSafeArea respects explicit drawOutside=false even when insets exist', () => {
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

  test('SendAndroidAutoDpi writes a positive dpi number into DPI file', () => {
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

  test('SendLogoType writes logo type as uint32 payload', () => {
    const msg = new SendLogoType(LogoType.Siri)
    const payload = msg.getPayload()

    expect(payload.readUInt32LE(0)).toBe(LogoType.Siri)
  })

  test('HeartBeat serialises header-only message', () => {
    const msg = new HeartBeat()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.HeartBeat)
  })

  test('SendCloseDongle serialises header-only message', () => {
    const msg = new SendCloseDongle()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.CloseDongle)
  })

  test('SendDisconnectPhone serialises header-only message', () => {
    const msg = new SendDisconnectPhone()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.DisconnectPhone)
  })

  test('SendNaviFocusRequest serialises header-only message', () => {
    const msg = new SendNaviFocusRequest()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.NaviFocusRequest)
  })

  test('SendNaviFocusRelease serialises header-only message', () => {
    const msg = new SendNaviFocusRelease()
    const buf = msg.serialise()

    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(8)).toBe(MessageType.NaviFocusRelease)
  })

  test('SendIconConfig includes oemIconLabel when oemName is provided', () => {
    const msg = new SendIconConfig({ oemName: 'My Car' })
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).toContain(`oemIconPath = ${FileAddress.OEM_ICON}`)
    expect(body).toContain('oemIconLabel = My Car')
  })

  test('SendIconConfig omits oemIconLabel when oemName is blank', () => {
    const msg = new SendIconConfig({ oemName: '   ' })
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).not.toContain('oemIconLabel =')
  })

  test('SendBoxSettings builds expected dashboard, gnss and fallback wifi fields', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 300,
        wifiChannel: Number.NaN,
        wifiType: '5ghz',
        mediaSound: 1,
        callQuality: 2,
        gps: true,
        autoConn: true,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 1,
        micType: 1,
        audioTransferMode: true,
        dashboardMediaInfo: true,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: true,
        gnssGps: true,
        gnssGlonass: false,
        gnssGalileo: true,
        gnssBeiDou: false,
        mapsEnabled: false
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
    expect(body.wifiName).toBe('CarName')
    expect(body.btName).toBe('CarName')
    expect(body.boxName).toBe('OEM')
    expect(body.OemName).toBe('OEM')
  })

  test('SendBoxSettings adds naviScreenInfo with computed safearea when maps are enabled', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 6,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: true,
        autoConn: false,
        UseBTPhone: true,
        carName: 'CarName',
        oemName: '',
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        mapsEnabled: true,
        naviWidth: 800,
        naviHeight: 480,
        naviFps: 30,
        naviSafeAreaTop: 10,
        naviSafeAreaBottom: 20,
        naviSafeAreaLeft: 30,
        naviSafeAreaRight: 40
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
        width: 730,
        height: 450,
        x: 30,
        y: 10,
        outside: 1
      }
    })
  })

  test('SendBoxSettings respects explicit naviSafeAreaDrawOutside=false', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: '',
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        mapsEnabled: true,
        naviWidth: 800,
        naviHeight: 480,
        naviFps: 24,
        naviSafeAreaTop: 10,
        naviSafeAreaBottom: 0,
        naviSafeAreaLeft: 0,
        naviSafeAreaRight: 0,
        naviSafeAreaDrawOutside: false
      } as any,
      1
    )

    const payload = msg.getPayload()
    const body = JSON.parse(payload.toString('ascii'))

    expect(body.naviScreenInfo.safearea.outside).toBe(0)
  })

  test('SendServerCgiScript targets LIVI_CGI and contains non-empty script', () => {
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

  test('SendLiviWeb targets LIVI_WEB and contains non-empty html payload', () => {
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

  test('SendRawMessage keeps type and raw payload', () => {
    const raw = new Uint8Array([0xde, 0xad, 0xbe, 0xef])
    const msg = new SendRawMessage(MessageType.DebugTrace, raw)

    expect(msg.type).toBe(MessageType.DebugTrace)
    expect(msg.getPayload()).toEqual(Buffer.from(raw))

    const buf = msg.serialise()
    expect(buf.readUInt32LE(0)).toBe(0x55aa55aa)
    expect(buf.readUInt32LE(4)).toBe(4)
    expect(buf.readUInt32LE(8)).toBe(MessageType.DebugTrace)
    expect(buf.subarray(16)).toEqual(Buffer.from(raw))
  })

  test('SendAutoConnectByBtAddress stores ascii bluetooth address payload', () => {
    const msg = new SendAutoConnectByBtAddress('AA:BB:CC:DD:EE:FF')

    expect(msg.type).toBe(MessageType.WifiStatusData)
    expect(msg.getPayload().toString('ascii')).toBe('AA:BB:CC:DD:EE:FF')
  })

  test('SendForgetBluetoothAddr stores ascii bluetooth address payload', () => {
    const msg = new SendForgetBluetoothAddr('11:22:33:44:55:66')

    expect(msg.type).toBe(MessageType.ForgetBluetoothAddr)
    expect(msg.getPayload().toString('ascii')).toBe('11:22:33:44:55:66')
  })

  test('SendBoxSettings logs payload when DEBUG is true', () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    jest.isolateModules(() => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      const { SendBoxSettings } = require('@main/services/projection/messages/sendable')

      const msg = new SendBoxSettings(
        {
          width: 1280,
          height: 720,
          fps: 60,
          mediaDelay: 0,
          wifiChannel: 1,
          wifiType: '2.4ghz',
          mediaSound: 1,
          callQuality: 1,
          gps: false,
          autoConn: false,
          UseBTPhone: false,
          carName: 'CarName',
          oemName: 'OEM',
          hand: 0,
          micType: 0,
          audioTransferMode: false,
          dashboardMediaInfo: false,
          dashboardVehicleInfo: false,
          dashboardRouteInfo: false,
          gnssGps: false,
          gnssGlonass: false,
          gnssGalileo: false,
          gnssBeiDou: false,
          mapsEnabled: false
        },
        123
      )

      const payload = msg.getPayload()
      const body = JSON.parse(payload.toString('ascii'))

      expect(body.syncTime).toBe(123)
    })

    expect(logSpy).toHaveBeenCalledWith('[SendBoxSettings]', expect.any(String))

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
  })

  test('SendBoxSettings logs payload when DEBUG is true', () => {
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    jest.resetModules()

    jest.isolateModules(() => {
      jest.doMock('@main/constants', () => ({
        DEBUG: true
      }))

      const { SendBoxSettings } = require('@main/services/projection/messages/sendable')

      const msg = new SendBoxSettings(
        {
          width: 1280,
          height: 720,
          fps: 60,
          mediaDelay: 0,
          wifiChannel: 1,
          wifiType: '2.4ghz',
          mediaSound: 1,
          callQuality: 1,
          gps: false,
          autoConn: false,
          UseBTPhone: false,
          carName: 'CarName',
          oemName: 'OEM',
          hand: 0,
          micType: 0,
          audioTransferMode: false,
          dashboardMediaInfo: false,
          dashboardVehicleInfo: false,
          dashboardRouteInfo: false,
          gnssGps: false,
          gnssGlonass: false,
          gnssGalileo: false,
          gnssBeiDou: false,
          mapsEnabled: false
        },
        123
      )

      const payload = msg.getPayload()
      const body = JSON.parse(payload.toString('ascii'))

      expect(body.syncTime).toBe(123)
    })

    expect(logSpy).toHaveBeenCalledWith('[SendBoxSettings]', expect.any(String))

    logSpy.mockRestore()
    jest.resetModules()
    jest.dontMock('@main/constants')
  })

  test('SendBoxSettings uses current time when syncTime is null', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        mapsEnabled: false
      } as any,
      null
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))
    expect(typeof body.syncTime).toBe('number')
    expect(body.syncTime).toBeGreaterThan(0)
  })

  test('SendBoxSettings falls back to carName when oemName is undefined', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: undefined,
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: true,
        mapsEnabled: false
      } as any,
      1
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))

    expect(body.boxName).toBe('CarName')
    expect(body.OemName).toBe('CarName')
    expect(body.GNSSCapability).toBe(8)
  })

  test('SendBoxSettings uses default navi safe-area zeros when values are undefined', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: 1,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: false,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: false,
        gnssGalileo: false,
        gnssBeiDou: false,
        mapsEnabled: true,
        naviWidth: 800,
        naviHeight: 480,
        naviFps: 30,
        naviSafeAreaTop: undefined,
        naviSafeAreaBottom: undefined,
        naviSafeAreaLeft: undefined,
        naviSafeAreaRight: undefined
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

  test('SendBoxSettings constructor uses default syncTime parameter when omitted', () => {
    const msg = new SendBoxSettings({
      width: 1280,
      height: 720,
      fps: 60,
      mediaDelay: 0,
      wifiChannel: 1,
      wifiType: '2.4ghz',
      mediaSound: 1,
      callQuality: 1,
      gps: false,
      autoConn: false,
      UseBTPhone: false,
      carName: 'CarName',
      oemName: 'OEM',
      hand: 0,
      micType: 0,
      audioTransferMode: false,
      dashboardMediaInfo: false,
      dashboardVehicleInfo: false,
      dashboardRouteInfo: false,
      gnssGps: false,
      gnssGlonass: false,
      gnssGalileo: false,
      gnssBeiDou: false,
      mapsEnabled: false
    } as any)

    const body = JSON.parse(msg.getPayload().toString('ascii'))
    expect(typeof body.syncTime).toBe('number')
    expect(body.syncTime).toBeGreaterThan(0)
  })

  test('SendIconConfig handles undefined oemName without label', () => {
    const msg = new SendIconConfig({})
    const payload = msg.getPayload()

    const nameLen = payload.readUInt32LE(0)
    const contentLen = payload.readUInt32LE(4 + nameLen)
    const body = payload.subarray(4 + nameLen + 4, 4 + nameLen + 4 + contentLen).toString('ascii')

    expect(body).toContain('oemIconVisible = 1')
    expect(body).not.toContain('oemIconLabel =')
  })

  test('SendGnssData treats nullish input as empty string', () => {
    const msg = new SendGnssData(undefined as any)
    expect(msg.getPayload().toString('ascii')).toBe('')
  })

  test('SendSafeArea uses default options and zero insets when omitted', () => {
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

  test('boxTmpPath falls back correctly for empty filename', () => {
    expect(boxTmpPath('')).toBe('/tmp/update.img')
  })

  test('SendBoxSettings uses 2.4ghz fallback channel and sets vehicle/glonass flags', () => {
    const msg = new SendBoxSettings(
      {
        width: 1280,
        height: 720,
        fps: 60,
        mediaDelay: 0,
        wifiChannel: Number.NaN,
        wifiType: '2.4ghz',
        mediaSound: 1,
        callQuality: 1,
        gps: false,
        autoConn: false,
        UseBTPhone: false,
        carName: 'CarName',
        oemName: 'OEM',
        hand: 0,
        micType: 0,
        audioTransferMode: false,
        dashboardMediaInfo: false,
        dashboardVehicleInfo: true,
        dashboardRouteInfo: false,
        gnssGps: false,
        gnssGlonass: true,
        gnssGalileo: false,
        gnssBeiDou: false,
        mapsEnabled: false
      } as any,
      1
    )

    const body = JSON.parse(msg.getPayload().toString('ascii'))

    expect(body.wifiChannel).toBe(1)
    expect(body.DashboardInfo).toBe(2)
    expect(body.GNSSCapability).toBe(2)
  })
})
