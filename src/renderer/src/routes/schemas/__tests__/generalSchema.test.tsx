import type { SettingsNode } from '../../types'
import { generalSchema } from '../generalSchema'

vi.mock('../../../components/pages/settings/pages/camera', () => ({ Camera: () => null }))
vi.mock('../../../components/pages/settings/pages/system/usbDongle/USBDongle', () => ({
  USBDongle: () => null
}))

type LoadFn = () => Promise<Array<{ value: unknown; label: string }>>

function collectLoaders(node: SettingsNode<unknown>): LoadFn[] {
  const out: LoadFn[] = []
  const walk = (n: Record<string, unknown>): void => {
    if (typeof n.loadOptions === 'function') out.push(n.loadOptions as LoadFn)
    if (Array.isArray(n.children)) for (const c of n.children) walk(c as Record<string, unknown>)
  }
  walk(node as unknown as Record<string, unknown>)
  return out
}

afterEach(() => {
  delete (window as unknown as { app?: unknown }).app
})

describe('generalSchema loadOptions', () => {
  const loaders = collectLoaders(generalSchema as unknown as SettingsNode<unknown>)

  test('the schema exposes the wifi/display/bt async loaders', () => {
    expect(loaders.length).toBeGreaterThanOrEqual(5)
  })

  test('each loader maps the bridge result into value/label options', async () => {
    ;(window as unknown as { app: unknown }).app = {
      listDisplayModes: vi.fn(async () => ['800x480', '1024x600']),
      listWifiChannels: vi.fn(async () => [36, 40]),
      listWifiCountryCodes: vi.fn(async () => ['DE', 'AT']),
      listWifiInterfaces: vi.fn(async () => ['wlan0']),
      listBtAdapters: vi.fn(async () => ['hci0'])
    }
    for (const load of loaders) {
      const opts = await load()
      expect(Array.isArray(opts)).toBe(true)
      expect(opts.length).toBeGreaterThan(0)
      for (const o of opts) expect(o).toHaveProperty('label')
    }
  })

  test('the dongle is labelled LIVI Link while other interfaces keep their name', async () => {
    ;(window as unknown as { app: unknown }).app = {
      listDisplayModes: vi.fn(async () => []),
      listWifiChannels: vi.fn(async () => []),
      listWifiCountryCodes: vi.fn(async () => []),
      listWifiInterfaces: vi.fn(async () => ['livi-link', 'wlan0']),
      listBtAdapters: vi.fn(async () => ['livi-link', 'hci0'])
    }
    const options = (await Promise.all(loaders.map((load) => load()))).flat() as Array<{
      value: string
      label?: string
    }>
    const labelFor = (value: string): string | undefined =>
      options.find((o) => o.value === value)?.label

    expect(labelFor('livi-link')).toBe('LIVI Link')
    expect(labelFor('wlan0')).toBe('wlan0')
    expect(labelFor('hci0')).toBe('hci0')
  })

  test('display modes keep the panel-default option ahead of the reported modes', async () => {
    ;(window as unknown as { app: unknown }).app = {
      listDisplayModes: vi.fn(async () => ['800x480'])
    }
    const displayNode = (generalSchema.children as SettingsNode<unknown>[])
      .flatMap((c) => (c.children ?? []) as SettingsNode<unknown>[])
      .flatMap((c) => (c.children ?? []) as SettingsNode<unknown>[])
      .find((n) => (n as { path?: string }).path === 'displayMode') as {
      loadOptions: LoadFn
    }
    const opts = await displayNode.loadOptions()
    expect(opts[0]).toMatchObject({ value: '', labelKey: 'settings.displayModeDefault' })
    expect(opts[opts.length - 1]).toMatchObject({ value: '800x480', label: '800x480' })
  })

  test('value transforms round-trip and format their values', () => {
    type VT = {
      toView: (v: number) => number
      fromView: (v: number) => number
      format: (v: number) => string
    }
    const transforms: VT[] = []
    const walk = (n: Record<string, unknown>): void => {
      if (n.valueTransform) transforms.push(n.valueTransform as VT)
      if (Array.isArray(n.children)) for (const c of n.children) walk(c as Record<string, unknown>)
    }
    walk(generalSchema as unknown as Record<string, unknown>)
    expect(transforms.length).toBeGreaterThanOrEqual(2)
    for (const t of transforms) {
      expect(t.toView(42)).toBe(42)
      expect(t.fromView(42)).toBe(42)
      expect(typeof t.format(42)).toBe('string')
      expect(t.format(42)).toContain('42')
    }
  })

  test('loaders fall back gracefully when the bridge is missing or returns non-arrays', async () => {
    delete (window as unknown as { app?: unknown }).app
    for (const load of loaders) {
      const opts = await load()
      expect(Array.isArray(opts)).toBe(true)
    }

    ;(window as unknown as { app: unknown }).app = {
      listDisplayModes: vi.fn(async () => null),
      listWifiChannels: vi.fn(async () => undefined),
      listWifiCountryCodes: vi.fn(async () => 'nope'),
      listWifiInterfaces: vi.fn(async () => 42),
      listBtAdapters: vi.fn(async () => ({}))
    }
    for (const load of loaders) {
      const opts = await load()
      expect(Array.isArray(opts)).toBe(true)
    }
  })
})
