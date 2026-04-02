import { generalSchema } from '../generalSchema'

const schema = generalSchema as any

describe('generalSchema', () => {
  test('exposes top-level general route with expected children', () => {
    expect(schema.type).toBe('route')
    expect(schema.route).toBe('general')
    expect(schema.label).toBe('General')
    expect(schema.labelKey).toBe('settings.general')
    expect(schema.path).toBe('')
    expect(schema.children).toHaveLength(12)
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

    expect(connections.children).toHaveLength(4)

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

  test('firmware settings route contains dashboard and gnss sections', () => {
    const firmware = schema.children[1]
    expect(firmware).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'firmwareSettings'
      })
    )
    expect(firmware.children).toHaveLength(2)

    const dashboard = firmware.children[0]
    expect(dashboard).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'dashboardInfo'
      })
    )
    expect(dashboard.children.map((x) => x.path)).toEqual([
      'dashboardMediaInfo',
      'dashboardVehicleInfo',
      'dashboardRouteInfo'
    ])

    const gnss = firmware.children[1]
    expect(gnss).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'gnss'
      })
    )
    expect(gnss.children.map((x) => x.path)).toEqual([
      'gps',
      'gnssGps',
      'gnssGlonass',
      'gnssGalileo',
      'gnssBeiDou'
    ])
  })

  test('auto switch route contains all three toggles', () => {
    const autoSwitch = schema.children[2]
    expect(autoSwitch).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'autoSwitch'
      })
    )
    expect(autoSwitch.children.map((x) => x.path)).toEqual([
      'autoSwitchOnStream',
      'autoSwitchOnPhoneCall',
      'autoSwitchOnGuidance'
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
    expect(bindingKeys).toContain('siri')
    expect(bindingKeys).toContain('siriRelease')
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
      { label: 'Maps', labelKey: 'settings.startPageMaps', value: 'maps' },
      { label: 'Telemetry', labelKey: 'settings.startPageTelemetry', value: 'telemetry' },
      { label: 'Media', labelKey: 'settings.startPageMedia', value: 'media' },
      { label: 'Camera', labelKey: 'settings.startPageCamera', value: 'camera' },
      { label: 'Settings', labelKey: 'settings.startPageSettings', value: 'settings' }
    ])
  })

  test('fft delay, steering wheel, telemetry, maps, fullscreen, zoom and language nodes are configured', () => {
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

    const telemetry = schema.children[7]
    expect(telemetry.type).toBe('route')
    expect(telemetry.route).toBe('telemetry')
    expect(telemetry.children[0]).toEqual(
      expect.objectContaining({
        type: 'posList',
        path: 'telemetryDashboards'
      })
    )
    expect(telemetry.children[0].items).toHaveLength(4)

    expect(schema.children[8]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'mapsEnabled'
      })
    )

    expect(schema.children[9]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'kiosk'
      })
    )
    expect(schema.children[10]).toEqual(
      expect.objectContaining({
        type: 'number',
        path: 'uiZoomPercent',
        displayValue: true,
        min: 50,
        max: 200,
        step: 10
      })
    )

    expect(schema.children[10].valueTransform?.toView?.(120)).toBe(120)
    expect(schema.children[10].valueTransform?.fromView?.(130)).toBe(130)
    expect(schema.children[10].valueTransform?.format?.(140)).toBe('140%')

    expect(schema.children[11]).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'language',
        displayValue: true
      })
    )
    expect(schema.children[11].options).toEqual([
      { label: 'English', labelKey: 'settings.english', value: 'en' },
      { label: 'German', labelKey: 'settings.german', value: 'de' },
      { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' }
    ])
  })
})
