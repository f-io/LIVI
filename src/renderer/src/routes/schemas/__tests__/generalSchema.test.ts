import { generalSchema } from '../generalSchema'

const schema = generalSchema as any

describe('generalSchema', () => {
  test('exposes top-level general route with expected children', () => {
    expect(schema.type).toBe('route')
    expect(schema.route).toBe('general')
    expect(schema.label).toBe('General')
    expect(schema.labelKey).toBe('settings.general')
    expect(schema.path).toBe('')
    expect(schema.children).toHaveLength(14)
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

  test('wireless projection lives at General top-level (sibling of connections)', () => {
    const wireless = schema.children[1]
    expect(wireless).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'wirelessProjection',
        label: 'Wireless Projection'
      })
    )
    expect(wireless.children[0]).toEqual(
      expect.objectContaining({
        type: 'checkbox',
        path: 'aa'
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

  test('dongle firmware settings route lives at the bottom with dashboard and gnss sections', () => {
    const firmware = schema.children[13]
    expect(firmware).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'dongleFirmwareSettings',
        label: 'Dongle Firmware Settings'
      })
    )
    expect(firmware.children).toHaveLength(3)

    const audioBuffer = firmware.children[0]
    expect(audioBuffer).toEqual(
      expect.objectContaining({
        type: 'number',
        path: 'mediaDelay',
        labelKey: 'settings.audioBufferSize'
      })
    )

    const dashboard = firmware.children[1]
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

    const gnss = firmware.children[2]
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
    const autoSwitch = schema.children[6]
    expect(autoSwitch).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'autoSwitch'
      })
    )
    expect(autoSwitch.children.map((x) => x.path)).toEqual([
      'autoSwitchOnStream',
      'autoSwitchOnPhoneCall',
      'autoSwitchOnGuidance',
      'autoSwitchOnReverse'
    ])
  })

  test('key bindings route contains representative binding entries', () => {
    const keyBindings = schema.children[7]
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
    const startPage = schema.children[8]
    expect(startPage).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'startPage',
        displayValue: true
      })
    )
    expect(startPage.options).toEqual([
      { label: 'Home', labelKey: 'settings.startPageHome', value: 'home' },
      { label: 'Cluster Stream', labelKey: 'settings.startPageCluster', value: 'cluster' },
      { label: 'Telemetry', labelKey: 'settings.startPageTelemetry', value: 'telemetry' },
      { label: 'Media', labelKey: 'settings.startPageMedia', value: 'media' },
      { label: 'Camera', labelKey: 'settings.startPageCamera', value: 'camera' },
      { label: 'Settings', labelKey: 'settings.startPageSettings', value: 'settings' }
    ])
  })

  test('window settings + media + dashboards live as siblings under General', () => {
    const windowSettings = schema.children[2]
    expect(windowSettings).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'windowSettings'
      })
    )
    const wsKeys = windowSettings.children.map((c) => (c.type === 'route' ? c.route : c.path))
    expect(wsKeys).toEqual(['mainScreen', 'dashScreen', 'auxScreen'])

    const mediaRoute = schema.children[3]
    expect(mediaRoute).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'media'
      })
    )
    expect(mediaRoute.children.map((c) => c.path)).toEqual([
      'media.main',
      'media.dash',
      'media.aux'
    ])

    const clusterRoute = schema.children[4]
    expect(clusterRoute).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'cluster'
      })
    )
    expect(clusterRoute.children.map((c) => c.path)).toEqual([
      'cluster.main',
      'cluster.dash',
      'cluster.aux'
    ])

    const dashboardsRoute = schema.children[5]
    expect(dashboardsRoute).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'dashboards'
      })
    )
    // 1 visible posList + 4 hidden per-dashboard sub-routes
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
      const paths = dashRoute.children.map((c) => c.path)
      expect(paths).toEqual([
        `dashboards.${id}.main`,
        `dashboards.${id}.dash`,
        `dashboards.${id}.aux`
      ])
    })
  })

  test('fft delay, steering wheel, fullscreen, zoom and language nodes are configured', () => {
    const fftDelay = schema.children[9]
    expect(fftDelay.type).toBe('number')
    expect(fftDelay.path).toBe('visualAudioDelayMs')
    expect(fftDelay.valueTransform?.toView?.(150)).toBe(150)
    expect(fftDelay.valueTransform?.fromView?.(160)).toBe(160)
    expect(fftDelay.valueTransform?.format?.(170)).toBe('170 ms')

    const steering = schema.children[10]
    expect(steering.type).toBe('select')
    expect(steering.path).toBe('hand')
    expect(steering.options).toEqual([
      { label: 'LHD', labelKey: 'settings.lhdr', value: 0 },
      { label: 'RHD', labelKey: 'settings.rhdr', value: 1 }
    ])

    expect(schema.children[11]).toEqual(
      expect.objectContaining({
        type: 'number',
        path: 'uiZoomPercent',
        displayValue: true,
        min: 50,
        max: 200,
        step: 10
      })
    )

    expect(schema.children[11].valueTransform?.toView?.(120)).toBe(120)
    expect(schema.children[11].valueTransform?.fromView?.(130)).toBe(130)
    expect(schema.children[11].valueTransform?.format?.(140)).toBe('140%')

    expect(schema.children[12]).toEqual(
      expect.objectContaining({
        type: 'select',
        path: 'language',
        displayValue: true
      })
    )
    expect(schema.children[12].options).toEqual([
      { label: 'English', labelKey: 'settings.english', value: 'en' },
      { label: 'German', labelKey: 'settings.german', value: 'de' },
      { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' }
    ])
  })
})
