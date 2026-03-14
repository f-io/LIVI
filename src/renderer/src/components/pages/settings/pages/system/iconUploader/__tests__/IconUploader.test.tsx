import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { IconUploader } from '../IconUploader'

const saveSettings = jest.fn()
const requestRestart = jest.fn()

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k })
}))

jest.mock('../utils', () => ({
  loadImageFromFile: jest.fn().mockResolvedValue({}),
  resizeImageToBase64Png: jest.fn((_: unknown, size: number) => `b64-${size}`)
}))

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: { dongleIcon120: '', dongleIcon180: '', dongleIcon256: '' },
      saveSettings
    }),
  useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleConnected: true })
}))

describe('IconUploader', () => {
  beforeEach(() => {
    saveSettings.mockClear()
    requestRestart.mockClear()
    ;(window as any).projection = {
      usb: { uploadIcons: jest.fn().mockResolvedValue(undefined) }
    }
    ;(window as any).app = {
      resetDongleIcons: jest.fn().mockResolvedValue({
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
        onChange={jest.fn()}
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
        onChange={jest.fn()}
        requestRestart={requestRestart}
      />
    )
    fireEvent.click(screen.getByText('settings.upload'))
    await waitFor(() => {
      expect((window as any).projection.usb.uploadIcons).toHaveBeenCalled()
      expect(requestRestart).toHaveBeenCalled()
    })
  })
})
