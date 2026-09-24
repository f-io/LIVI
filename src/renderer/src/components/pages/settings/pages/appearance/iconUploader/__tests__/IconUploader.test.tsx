import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { IconUploader } from '../IconUploader'

const saveSettings = vi.fn()
const requestRestart = vi.fn()

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k })
}))

vi.mock('../utils', () => ({
  loadImageFromFile: vi.fn().mockResolvedValue({}),
  resizeImageToBase64Png: vi.fn((_: unknown, size: number) => `b64-${size}`)
}))

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: { carplayIcon120: '', carplayIcon180: '', carplayIcon256: '' },
      saveSettings
    })
}))

describe('IconUploader', () => {
  let warnSpy: ReturnType<typeof vi.spyOn>

  beforeEach(async () => {
    warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    saveSettings.mockClear()
    requestRestart.mockClear()
  })

  afterEach(() => {
    warnSpy.mockRestore()
  })

  test('imports png and saves resized icon fields', async () => {
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const file = new File(['x'], 'icon.png', { type: 'image/png' })

    fireEvent.change(input, { target: { files: [file] } })
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalled()
    })
  })

  test('reset clears the icons, so the built-in logo applies again', async () => {
    render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    fireEvent.click(screen.getByText('settings.reset'))
    await waitFor(() => {
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          carplayIcon120: '',
          carplayIcon180: '',
          carplayIcon256: ''
        })
      )
    })
  })

  test('import failure warns and saves nothing', async () => {
    const { loadImageFromFile } = await import('../utils')
    loadImageFromFile.mockRejectedValueOnce(new Error('bad file'))
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    fireEvent.change(input, {
      target: { files: [new File(['x'], 'bad.png', { type: 'image/png' })] }
    })
    await waitFor(() => {
      expect(warnSpy).toHaveBeenCalledWith('[IconUploader] import failed', expect.any(Error))
    })
    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('shows icon preview when carplayIcon180 is set', async () => {
    vi.resetModules()
    vi.doMock('@store/store', () => ({
      useLiviStore: (selector: (s: any) => unknown) =>
        selector({
          settings: { carplayIcon120: '', carplayIcon180: 'abc', carplayIcon256: '' },
          saveSettings
        }),
      useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleHardwarePresent: true })
    }))
    const { IconUploader: FreshIconUploader } = await import('../IconUploader')

    render(
      <FreshIconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    // carplayIcon180 is set, so the placeholder is not shown
    expect(screen.queryByText('No icon found')).not.toBeInTheDocument()

    vi.doUnmock('@store/store')
  })

  test('keyboard enter on icon box triggers file picker', async () => {
    render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const iconBox = screen.getByLabelText('icon preview')
    fireEvent.keyDown(iconBox, { key: 'Enter' })
    expect(iconBox).toBeInTheDocument()
  })

  test('clicking the icon box opens the hidden file picker', async () => {
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const clickSpy = vi.spyOn(input, 'click')
    const iconBox = screen.getByLabelText('icon preview')
    fireEvent.click(iconBox)
    expect(clickSpy).toHaveBeenCalled()
  })

  test('space key on icon box opens the file picker', async () => {
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const clickSpy = vi.spyOn(input, 'click')
    const iconBox = screen.getByLabelText('icon preview')
    fireEvent.keyDown(iconBox, { key: ' ' })
    expect(clickSpy).toHaveBeenCalled()
  })

  test('no file selected leaves settings untouched', async () => {
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    fireEvent.change(input, { target: { files: [] } })
    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('other keys on the icon box do not open the picker', async () => {
    const { container } = render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    const input = container.querySelector('input[type="file"]') as HTMLInputElement
    const clickSpy = vi.spyOn(input, 'click')
    fireEvent.keyDown(screen.getByLabelText('icon preview'), { key: 'a' })
    expect(clickSpy).not.toHaveBeenCalled()
  })

  test('shows the placeholder when no icon data is available anywhere', async () => {
    vi.resetModules()
    vi.doMock('@shared/assets/carIcons', () => ({
      ICON_120_B64: '',
      ICON_180_B64: '',
      ICON_256_B64: ''
    }))
    vi.doMock('@store/store', () => ({
      useLiviStore: (selector: (s: any) => unknown) =>
        selector({
          settings: { carplayIcon120: '', carplayIcon180: '', carplayIcon256: '' },
          saveSettings
        }),
      useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleHardwarePresent: true })
    }))
    const { IconUploader: FreshIconUploader } = await import('../IconUploader')

    render(
      <FreshIconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    expect(screen.getByText('No icon found')).toBeInTheDocument()

    vi.doUnmock('@shared/assets/carIcons')
    vi.doUnmock('@store/store')
  })

  test('renders nothing when settings are missing', async () => {
    vi.resetModules()
    vi.doMock('@store/store', () => ({
      useLiviStore: (selector: (s: any) => unknown) => selector({ settings: null, saveSettings }),
      useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleHardwarePresent: true })
    }))
    const { IconUploader: FreshIconUploader } = await import('../IconUploader')

    const { container } = render(
      <FreshIconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    expect(container.firstChild).toBeNull()

    vi.doUnmock('@store/store')
  })
})
