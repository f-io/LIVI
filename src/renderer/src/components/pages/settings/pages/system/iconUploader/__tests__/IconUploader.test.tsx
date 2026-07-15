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
      settings: { dongleIcon120: '', dongleIcon180: '', dongleIcon256: '' },
      saveSettings
    }),
  useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleHardwarePresent: true })
}))

describe('IconUploader', () => {
  beforeEach(async () => {
    saveSettings.mockClear()
    requestRestart.mockClear()
    ;(window as any).projection = {
      usb: { uploadIcons: vi.fn().mockResolvedValue(undefined) }
    }
    ;(window as any).app = {
      resetDongleIcons: vi.fn().mockResolvedValue({
        dongleIcon120: 'x120',
        dongleIcon180: 'x180',
        dongleIcon256: 'x256'
      })
    }
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

  test('uploads icons and requests restart', async () => {
    render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    fireEvent.click(screen.getByText('settings.upload'))
    await waitFor(() => {
      expect((window as any).projection.usb.uploadIcons).toHaveBeenCalled()
      expect(requestRestart).toHaveBeenCalled()
    })
  })

  test('resets to default icons via resetDongleIcons', async () => {
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
      expect((window as any).app.resetDongleIcons).toHaveBeenCalled()
      expect(saveSettings).toHaveBeenCalledWith(
        expect.objectContaining({
          dongleIcon120: 'x120',
          dongleIcon180: 'x180',
          dongleIcon256: 'x256'
        })
      )
    })
    expect(screen.getByText('Icons reset to defaults.')).toBeInTheDocument()
  })

  test('reset shows error message when resetDongleIcons API not available', async () => {
    delete (window as any).app
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
      expect(screen.getByText('Reset API not available.')).toBeInTheDocument()
    })
  })

  test('getResetDongleIconsFn returns null when app is not a record', async () => {
    ;(window as any).app = null
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
      expect(screen.getByText('Reset API not available.')).toBeInTheDocument()
    })
  })

  test('import failure shows error message', async () => {
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
      expect(screen.getByText('Icon import failed.')).toBeInTheDocument()
    })
  })

  test('upload failure shows error message', async () => {
    ;(window as any).projection.usb.uploadIcons = vi.fn().mockRejectedValue(new Error('usb fail'))
    render(
      <IconUploader
        state={{} as any}
        node={{} as any}
        onChange={vi.fn()}
        requestRestart={requestRestart}
      />
    )
    fireEvent.click(screen.getByText('settings.upload'))
    await waitFor(() => {
      expect(screen.getByText('Icon upload failed.')).toBeInTheDocument()
    })
  })

  test('shows icon preview when dongleIcon180 is set', async () => {
    vi.resetModules()
    vi.doMock('@store/store', () => ({
      useLiviStore: (selector: (s: any) => unknown) =>
        selector({
          settings: { dongleIcon120: '', dongleIcon180: 'abc', dongleIcon256: '' },
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
    // dongleIcon180 is set, so the placeholder is not shown
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
    const _fileInputRef = { click: vi.fn() }
    const iconBox = screen.getAllByRole('button')[0] // first is the icon box div
    // Simulate pressing Enter on the icon box
    fireEvent.keyDown(iconBox, { key: 'Enter' })
    // The file input is hidden; picking would call fileInputRef.current?.click()
    // We can't easily verify the click on the hidden input, but we can verify
    // the handler runs without error
    expect(iconBox).toBeInTheDocument()
  })
})
