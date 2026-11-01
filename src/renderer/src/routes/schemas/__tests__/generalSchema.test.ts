import { generalSchema } from '../generalSchema'

const schema = generalSchema as any

describe('generalSchema', () => {
  test('exposes top-level general route with expected children', () => {
    expect(schema.type).toBe('route')
    expect(schema.route).toBe('general')
    expect(schema.label).toBe('General')
    expect(schema.labelKey).toBe('settings.general')
    expect(schema.path).toBe('')
    expect(schema.children).toHaveLength(13)
    expect(schema.children[12]).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'vehicleTelemetry',
        labelKey: 'settings.vehicleTelemetry'
      })
    )
    expect(schema.children[12].children[0]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'haltechCanEnabled',
        labelKey: 'settings.haltechCanEnabled'
      })
    )
  })

  test('connections route contains names, wifi and auto connect', () => {
    const connections = schema.children[0]
    expect(connections).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'connections',
        label: 'Connections'
      })
    )

    expect(connections.children).toHaveLength(6)

    expect(connections.children[0]).toEqual(
      expect.objectContaining({
        type: 'string',
        path: 'carName'
      })
    )
    expect(connections.children[1]).toEqual(
      expect.objectContaining({
        type: 'string',
        path: 'oemName'
      })
    )
    expect(connections.children[3]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'wirelessAaEnabled'
      })
    )
    expect(connections.children[4]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'wirelessCpEnabled'
      })
    )
    expect(connections.children[5]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'autoConn'
      })
    )
  })

  test('wifi route contains expected frequency options', () => {
    const wifi = schema.children[0].children[2]
    expect(wifi).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'wifi'
      })
    )

    const select = wifi.children[0]
    expect(select).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'wifiType',
        displayValue: true
      })
    )
    expect(select.options).toEqual([
      { label: '2.4 GHz', value: '2.4ghz' },
      { label: '5 GHz', value: '5ghz' }
    ])
  })

  test('mfi route is a top-level entry with cp gen, i2c bus and power pin', () => {
    const mfi = schema.children[9]
    expect(mfi).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'mfi',
        labelKey: 'settings.mfi'
      })
    )
    expect(mfi.children.map((x) => x.path)).toEqual(['carPlayMfiI2cBus', 'carPlayMfiPowerGpio'])
  })

  test('usb dongle route contains a single custom entry', () => {
    const usbDongle = schema.children[10]
    expect(usbDongle).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'usbDongle',
        labelKey: 'settings.usbDongle'
      })
    )
    expect(usbDongle.children).toHaveLength(1)
    expect(usbDongle.children[0]).toEqual(
      expect.objectContaining({
        type: 'custom',
        path: 'carName'
      })
    )
  })

  test('games route contains RetroArch library settings', () => {
    const games = schema.children[11]
    expect(games).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'games',
        label: 'Games'
      })
    )
    expect(games.children.map((child) => child.path)).toEqual([
      'games.enabled',
      'games.retroArchPath',
      'games.romDirectory',
      'games.playlistDirectory',
      'games.thumbnailDirectory'
    ])
  })

  test('key bindings route contains representative binding entries', () => {
    const keyBindings = schema.children[3]
    expect(keyBindings).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'keyBindings'
      })
    )

    const bindingKeys = keyBindings.children.map((x) => x.bindingKey)
    expect(bindingKeys).toContain('up')
    expect(bindingKeys).toContain('down')
    expect(bindingKeys).toContain('left')
    expect(bindingKeys).toContain('right')
    expect(bindingKeys).toContain('home')
    expect(bindingKeys).toContain('playPause')
    expect(bindingKeys).toContain('acceptPhone')
    expect(bindingKeys).toContain('rejectPhone')
    expect(bindingKeys).toContain('voiceAssistant')
    expect(bindingKeys).toContain('voiceAssistantRelease')
  })

  test('start page select exposes all expected page options', () => {
    const startPage = schema.children[4]
    expect(startPage).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'startPage',
        displayValue: true
      })
    )
    expect(startPage.options).toEqual([
      { label: 'Home', labelKey: 'settings.startPageHome', value: 'home' },
      { label: 'Telemetry', labelKey: 'settings.startPageTelemetry', value: 'telemetry' },
      { label: 'Media', labelKey: 'settings.startPageMedia', value: 'media' },
      { label: 'Camera', labelKey: 'settings.startPageCamera', value: 'camera' },
      { label: 'Games', value: 'games' },
      { label: 'MiniDSP', value: 'minidsp' },
      { label: 'Settings', labelKey: 'settings.startPageSettings', value: 'settings' }
    ])
  })

  test('window settings + tab settings live as siblings, tab settings hosts dashboards/media/camera', () => {
    const windowSettings = schema.children[1]
    expect(windowSettings).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'windowSettings'
      })
    )
    expect(windowSettings.children.map((c) => c.route)).toEqual([
      'mainScreen',
      'dashScreen',
      'auxScreen'
    ])

    const tabSettings = schema.children[2]
    expect(tabSettings).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'tabSettings'
      })
    )
    const tabRoutes = tabSettings.children.map((c) => c.route)
    expect(tabRoutes).toEqual(['dashboards', 'media', 'camera'])

    const dashboardsRoute = tabSettings.children[0]
    expect(dashboardsRoute.children).toHaveLength(5)
    const posList = dashboardsRoute.children[0]
    expect(posList).toEqual(
      expect.objectContaining({
        type: 'posList',
        path: 'dashboards'
      })
    )
    expect(posList.items.map((it) => it.id)).toEqual(['dash1', 'dash2', 'dash3', 'dash4'])

    const ids = ['dash1', 'dash2', 'dash3', 'dash4']
    ids.forEach((id, i) => {
      const dashRoute = dashboardsRoute.children[i + 1]
      expect(dashRoute).toEqual(
        expect.objectContaining({
          type: 'route',
          route: id,
          hidden: true
        })
      )
      expect(dashRoute.children.map((c) => c.path)).toEqual([
        `dashboards.${id}.main`,
        `dashboards.${id}.dash`,
        `dashboards.${id}.aux`
      ])
    })

    const mediaRoute = tabSettings.children[1]
    expect(mediaRoute.children.map((c) => c.path)).toEqual([
      'media.main',
      'media.dash',
      'media.aux'
    ])

    const cameraRoute = tabSettings.children[2]
    expect(cameraRoute.children.map((c) => (c.type === 'route' ? c.route : c.path))).toEqual([
      'autoSwitchOnReverse',
      'camera.main',
      'camera.dash',
      'camera.aux',
      'cameraMirror',
      'cameraRotation',
      'select'
    ])
  })

  test('fft delay, steering wheel, fullscreen, zoom and language nodes are configured', () => {
    const fftDelay = schema.children[5]
    expect(fftDelay.type).toBe('number')
    expect(fftDelay.path).toBe('visualAudioDelayMs')
    expect(fftDelay.valueTransform?.toView?.(150)).toBe(150)
    expect(fftDelay.valueTransform?.fromView?.(160)).toBe(160)
    expect(fftDelay.valueTransform?.format?.(170)).toBe('170 ms')

    const steering = schema.children[6]
    expect(steering.type).toBe('select')
    expect(steering.path).toBe('hand')
    expect(steering.options).toEqual([
      { label: 'LHD', labelKey: 'settings.lhdr', value: 0 },
      { label: 'RHD', labelKey: 'settings.rhdr', value: 1 }
    ])

    expect(schema.children[7]).toEqual(
      expect.objectContaining({
        type: 'number',
        path: 'uiZoomPercent',
        displayValue: true,
        min: 50,
        max: 200,
        step: 10
      })
    )

    expect(schema.children[7].valueTransform?.toView?.(120)).toBe(120)
    expect(schema.children[7].valueTransform?.fromView?.(130)).toBe(130)
    expect(schema.children[7].valueTransform?.format?.(140)).toBe('140%')

    expect(schema.children[8]).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'language',
        displayValue: true
      })
    )
    expect(schema.children[8].options).toEqual([
      { label: 'English', labelKey: 'settings.english', value: 'en' },
      { label: 'German', labelKey: 'settings.german', value: 'de' },
      { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' },
      { label: 'French', labelKey: 'settings.french', value: 'fr' }
    ])
  })
})
