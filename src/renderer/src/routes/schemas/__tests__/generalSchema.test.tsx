import { useLiviStore } from '../../../store/store'
import type { SettingsNode } from '../../types'
import { generalSchema } from '../generalSchema'

const coreActionMock = vi.fn((_action: unknown) => Promise.resolve())

vi.mock('../../../store/store', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../store/store')>()),
  coreAction: (action: unknown) => coreActionMock(action)
}))
vi.mock('../../../components/pages/settings/pages/camera', () => ({ Camera: () => null }))

const system = (
  wifiInterfaces: string[],
  btAdapters: string[],
  dongle: { wifi: boolean; bt: boolean } | null = null,
  wifiChannels: number[] = [36, 40],
  wifiCountries: string[] = ['DE', 'AT'],
  displayModes: string[] = ['800x480', '1024x600'],
  wifiBands: Array<'2.4ghz' | '5ghz' | '6ghz'> = [],
  interfaceModels: Record<string, string> = {}
) =>
  useLiviStore.setState({
    system: {
      wifiInterfaces,
      btAdapters,
      interfaceModels,
      dongle,
      linkSpeed: null,
      wifiBands,
      wifiChannels,
      wifiCountries,
      displayModes,
      displayModeSettable: true,
      audioSinks: [],
      audioSources: []
    }
  })
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
  useLiviStore.setState({ system: null })
  coreActionMock.mockClear()
})

describe('generalSchema loadOptions', () => {
  const loaders = collectLoaders(generalSchema as unknown as SettingsNode<unknown>)

  test('the schema exposes the wifi/display/bt async loaders', () => {
    expect(loaders.length).toBeGreaterThanOrEqual(5)
  })

  test('each loader maps the system lists into value/label options', async () => {
    system(['wlan0'], ['hci0'])
    for (const load of loaders) {
      const opts = await load()
      expect(Array.isArray(opts)).toBe(true)
      expect(opts.length).toBeGreaterThan(0)
      for (const o of opts) expect(o).toHaveProperty('label')
    }
  })

  test('the dongle is labelled LIVI Link while other interfaces keep their name', async () => {
    system(['livi-link', 'wlan0'], ['livi-link', 'hci0'], { wifi: true, bt: true })
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

  test('a local interface names its chip where the system can tell', async () => {
    system(['wlp104s0', 'wlan0'], ['hci1', 'hci0'], null, [], [], [], [], {
      wlp104s0: 'MT7925',
      hci1: 'MT7925'
    })
    const options = (await Promise.all(loaders.map((load) => load()))).flat() as Array<{
      value: string
      label?: string
    }>
    const labelFor = (value: string): string | undefined =>
      options.find((o) => o.value === value)?.label

    expect(labelFor('wlp104s0')).toBe('wlp104s0 (MT7925)')
    expect(labelFor('wlan0')).toBe('wlan0')
    expect(labelFor('hci1')).toBe('hci1 (MT7925)')
    expect(labelFor('hci0')).toBe('hci0')
  })

  type AdapterSelect = {
    loadOptions: () => Promise<Array<{ value: string; label: string; labelKey?: string }>>
    onPick: (value: string) => void
  }

  function adapterSelect(path: 'wifiInterface' | 'btAdapter'): AdapterSelect {
    let found: AdapterSelect | undefined
    const walk = (n: Record<string, unknown>): void => {
      if (n.type === 'select' && n.path === path) found = n as unknown as AdapterSelect
      if (Array.isArray(n.children)) for (const c of n.children) walk(c as Record<string, unknown>)
    }
    walk(generalSchema as unknown as Record<string, unknown>)
    if (!found) throw new Error(`no ${path} select`)
    return found
  }

  test('the dongle says so where the radio it would serve is switched off', async () => {
    system(['livi-link'], ['livi-link'], { wifi: false, bt: true })

    expect((await adapterSelect('wifiInterface').loadOptions())[0]).toMatchObject({
      label: 'LIVI Link (off)',
      labelKey: 'settings.dongleSwitchedOff'
    })
    expect((await adapterSelect('btAdapter').loadOptions())[0]).toEqual({
      value: 'livi-link',
      label: 'LIVI Link'
    })
  })

  test('picking the dongle switches its radio on, picking anything else leaves it alone', () => {
    coreActionMock.mockReturnValueOnce(Promise.reject(new Error('no dongle')))

    adapterSelect('wifiInterface').onPick('livi-link')
    adapterSelect('btAdapter').onPick('livi-link')
    adapterSelect('wifiInterface').onPick('wlan0')
    adapterSelect('btAdapter').onPick('hci0')

    expect(coreActionMock.mock.calls).toEqual([
      [{ kind: 'setDongleRadio', radio: 'wifi', on: true }],
      [{ kind: 'setDongleRadio', radio: 'bt', on: true }]
    ])
  })

  test('the band choice offers what the radio can run an access point on', async () => {
    let bands: { loadOptions: LoadFn } | undefined
    const walk = (n: Record<string, unknown>): void => {
      if (n.path === 'wifiType') bands = n as unknown as { loadOptions: LoadFn }
      if (Array.isArray(n.children)) for (const c of n.children) walk(c as Record<string, unknown>)
    }
    walk(generalSchema as unknown as Record<string, unknown>)

    system(['wlan0'], [], null, [37], ['DE'], [], ['2.4ghz', '5ghz', '6ghz'])
    expect(await bands!.loadOptions()).toEqual([
      { value: '2.4ghz', label: '2.4 GHz' },
      { value: '5ghz', label: '5 GHz' },
      { value: '6ghz', label: '6 GHz' }
    ])

    system(['wlan0'], [])
    expect((await bands!.loadOptions()).map((o) => o.value)).toEqual(['2.4ghz', '5ghz'])
  })

  test('display modes keep the panel-default option ahead of the reported modes', async () => {
    system([], [], null, [], [], ['1024x600', '800x480'])
    const displayNode = (generalSchema.children as SettingsNode<unknown>[])
      .flatMap((c) => (c.children ?? []) as SettingsNode<unknown>[])
      .flatMap((c) => (c.children ?? []) as SettingsNode<unknown>[])
      .find((n) => (n as { path?: string }).path === 'displayMode') as {
      loadOptions: LoadFn
      readOnly: (system: { displayModeSettable: boolean } | null) => boolean
    }
    const opts = await displayNode.loadOptions()
    expect(opts[0]).toMatchObject({ value: '', labelKey: 'settings.displayModeDefault' })
    expect(opts[opts.length - 1]).toMatchObject({ value: '800x480', label: '800x480' })

    // Under GNOME the modes are only shown
    expect(displayNode.readOnly({ displayModeSettable: true })).toBe(false)
    expect(displayNode.readOnly({ displayModeSettable: false })).toBe(true)
    expect(displayNode.readOnly(null)).toBe(true)
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

  test('loaders fall back gracefully before core listed anything', async () => {
    for (const load of loaders) {
      const opts = await load()
      expect(Array.isArray(opts)).toBe(true)
    }
  })
})
