import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { USBDongle } from '../USBDongle'

let onEventCb: ((e: unknown, p: unknown) => void) | undefined

type DevListItem = {
  index: number
  name: string
  type: string
  id: string
  time: string
  rfcomm: string
}

const state = {
  isDongleConnected: true,
  isStreaming: false,
  settings: { dongleToolsIp: '' },
  saveSettings: jest.fn().mockResolvedValue(undefined),
  vendorId: 0x1234,
  productId: 0xabcd,
  usbFwVersion: '1.0.0',
  dongleFwVersion: '2025.01.01.0001',
  boxInfo: {
    uuid: 'u1',
    MFD: 'mfd',
    productType: 'p1',
    DevList: [] as DevListItem[]
  },
  negotiatedWidth: 1280,
  negotiatedHeight: 720,
  audioCodec: 'aac',
  audioSampleRate: 48000,
  audioChannels: 2,
  audioBitDepth: 16
}

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string, vars?: Record<string, unknown>) => {
      if (!vars) return k
      return `${k} ${JSON.stringify(vars)}`
    }
  })
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) =>
    selector({ isDongleConnected: state.isDongleConnected, isStreaming: state.isStreaming }),
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: state.settings,
      saveSettings: state.saveSettings,
      vendorId: state.vendorId,
      productId: state.productId,
      usbFwVersion: state.usbFwVersion,
      dongleFwVersion: state.dongleFwVersion,
      boxInfo: state.boxInfo,
      negotiatedWidth: state.negotiatedWidth,
      negotiatedHeight: state.negotiatedHeight,
      audioCodec: state.audioCodec,
      audioSampleRate: state.audioSampleRate,
      audioChannels: state.audioChannels,
      audioBitDepth: state.audioBitDepth
    })
}))

jest.mock('@renderer/hooks/useNetworkStatus', () => ({
  useNetworkStatus: jest.fn(() => ({ online: true, type: 'wifi', effectiveType: '4g' }))
}))

describe('USBDongle', () => {
  beforeEach(() => {
    jest.clearAllMocks()
    state.isDongleConnected = true
    state.isStreaming = false
    state.settings = { dongleToolsIp: '' }
    state.boxInfo = { uuid: 'u1', MFD: 'mfd', productType: 'p1', DevList: [] }
    state.dongleFwVersion = '2025.01.01.0001'
    state.saveSettings.mockResolvedValue(undefined)
    onEventCb = undefined
    ;(window as any).projection = {
      ipc: {
        dongleFirmware: jest.fn(async (action: string) => ({
          ok: true,
          raw: { err: 0, ver: action === 'check' ? '2025.02.01.0001' : '-' },
          request: { local: { ok: true, ready: false, reason: 'missing' } }
        })),
        onEvent: jest.fn((cb: any) => {
          onEventCb = cb
        }),
        offEvent: jest.fn()
      },
      usb: {
        uploadLiviScripts: jest
          .fn()
          .mockResolvedValue({ ok: true, cgiOk: true, webOk: true, urls: [] })
      }
    }
    ;(window as any).app = {
      openExternal: jest.fn().mockResolvedValue({ ok: true })
    }
  })

  test('renders status sections and runs firmware check action', async () => {
    render(<USBDongle />)

    expect(screen.getByText('Status')).toBeInTheDocument()
    expect(screen.getByText('Firmware')).toBeInTheDocument()
    expect(screen.getByText('Check for Updates')).toBeInTheDocument()

    await waitFor(() => {
      expect((window as any).projection.ipc.dongleFirmware).toHaveBeenCalledWith('status')
    })

    fireEvent.click(screen.getByText('Check for Updates'))

    await waitFor(() => {
      expect((window as any).projection.ipc.dongleFirmware).toHaveBeenCalledWith('check')
    })
  })

  test('shows fw progress dialog when fwUpdate events are received', async () => {
    render(<USBDongle />)

    act(() => {
      onEventCb?.(null, { type: 'fwUpdate', stage: 'download:start' })
      onEventCb?.(null, {
        type: 'fwUpdate',
        stage: 'download:progress',
        received: 1024,
        total: 2048,
        percent: 0.5
      })
    })

    expect(screen.getByText('Dongle Firmware')).toBeInTheDocument()
    expect(screen.getByText('Downloading')).toBeInTheDocument()
    expect(screen.getByText('50% • 1 KB / 2 KB')).toBeInTheDocument()
  })

  test('renders device list entries from box info', () => {
    state.boxInfo = {
      uuid: 'u1',
      MFD: 'mfd',
      productType: 'p1',
      DevList: [
        {
          index: 1,
          name: 'Phone A',
          type: 'CarPlay',
          id: 'AA:BB',
          time: '123',
          rfcomm: '5'
        }
      ]
    }

    render(<USBDongle />)

    expect(screen.getByText('Device 1:')).toBeInTheDocument()
    expect(screen.getByText('Phone A')).toBeInTheDocument()
    expect(screen.getByText('CarPlay')).toBeInTheDocument()
    expect(screen.getByText('AA:BB')).toBeInTheDocument()
  })

  test('renders fallback device list row when no devices exist', () => {
    state.boxInfo = {
      uuid: 'u1',
      MFD: 'mfd',
      productType: 'p1',
      DevList: [] as DevListItem[]
    }

    render(<USBDongle />)

    expect(screen.getByLabelText('Device List: —')).toBeInTheDocument()
  })

  test('shows changelog button enabled and opens vendor changelog dialog', async () => {
    ;(window as any).projection.ipc.dongleFirmware = jest.fn(async (action: string) => ({
      ok: true,
      raw: {
        err: 0,
        ver: action === 'check' ? '2025.02.01.0001' : '-',
        notes: 'Bug fixes\nImprovements'
      },
      request: { local: { ok: true, ready: false, reason: 'missing' } }
    }))

    render(<USBDongle />)

    const changelogBtn = await screen.findByText('Changelog')
    expect(changelogBtn).not.toBeDisabled()

    fireEvent.click(changelogBtn)

    expect(screen.getByText('Vendor changelog')).toBeInTheDocument()
    expect(screen.getByText((content) => content.includes('Bug fixes'))).toBeInTheDocument()
    expect(screen.getByText((content) => content.includes('Improvements'))).toBeInTheDocument()
  })

  test('download button opens ready dialog immediately when firmware is already downloaded', async () => {
    ;(window as any).projection.ipc.dongleFirmware = jest.fn(async (action: string) => {
      if (action === 'status' || action === 'download') {
        return {
          ok: true,
          raw: { err: 0, ver: '2025.02.01.0001' },
          request: {
            local: {
              ok: true,
              ready: true,
              path: '/tmp/fw.bin',
              bytes: 4096,
              latestVer: '2025.02.01.0001'
            }
          }
        }
      }

      return {
        ok: true,
        raw: { err: 0, ver: '-' },
        request: { local: { ok: true, ready: false, reason: 'missing' } }
      }
    })

    render(<USBDongle />)

    await waitFor(() => {
      expect((window as any).projection.ipc.dongleFirmware).toHaveBeenCalledWith('status')
    })

    fireEvent.click(screen.getByText('Download'))

    await waitFor(() => {
      expect(screen.getByText('Dongle Firmware')).toBeInTheDocument()
      expect(screen.getByText(/Already downloaded\./)).toBeInTheDocument()
    })
  })

  test('upload button is disabled when local firmware is not ready', async () => {
    ;(window as any).projection.ipc.dongleFirmware = jest.fn(async () => ({
      ok: true,
      raw: { err: 0, ver: '2025.02.01.0001' },
      request: {
        local: {
          ok: true,
          ready: false,
          reason: 'missing'
        }
      }
    }))

    render(<USBDongle />)

    const uploadBtn = await screen.findByText('Upload')
    expect(uploadBtn).toBeDisabled()
  })

  test('enables dev tools, saves configured IP and opens matching URL', async () => {
    ;(window as any).projection.usb.uploadLiviScripts = jest.fn().mockResolvedValue({
      ok: true,
      cgiOk: true,
      webOk: true,
      urls: ['http://192.168.1.10/cgi-bin/server.cgi?action=ls&path=/']
    })

    render(<USBDongle />)

    const input = screen.getByLabelText('settings.dongleIpOptional') as HTMLInputElement
    fireEvent.change(input, { target: { value: '192.168.1.10' } })

    fireEvent.click(screen.getByText('settings.enableDevTools'))

    await waitFor(() => {
      expect(state.saveSettings).toHaveBeenCalledWith({ dongleToolsIp: '192.168.1.10' })
    })

    await waitFor(() => {
      expect((window as any).projection.usb.uploadLiviScripts).toHaveBeenCalled()
    })

    await waitFor(() => {
      expect((window as any).app.openExternal).toHaveBeenCalledWith(
        'http://192.168.1.10/index.html'
      )
    })
  })

  test('shows error when dev tools IP is invalid', async () => {
    render(<USBDongle />)

    const input = screen.getByLabelText('settings.dongleIpOptional') as HTMLInputElement
    fireEvent.change(input, { target: { value: '999.999.1.1' } })

    fireEvent.click(screen.getByText('settings.enableDevTools'))

    await waitFor(() => {
      expect(screen.getAllByText(/settings.devToolsInvalidIp/).length).toBeGreaterThan(0)
    })

    expect((window as any).projection.usb.uploadLiviScripts).not.toHaveBeenCalled()
  })

  test('shows partial alert when dev tools upload succeeds only partially', async () => {
    ;(window as any).projection.usb.uploadLiviScripts = jest.fn().mockResolvedValue({
      ok: false,
      cgiOk: true,
      webOk: false,
      urls: []
    })

    render(<USBDongle />)

    fireEvent.click(screen.getByText('settings.enableDevTools'))

    await waitFor(() => {
      expect(screen.getByText(/settings.devToolsPartial/)).toBeInTheDocument()
    })
  })

  test('shows candidate URLs when no URL was opened but upload returned candidates', async () => {
    state.settings = { dongleToolsIp: '' }
    ;(window as any).projection.usb.uploadLiviScripts = jest.fn().mockResolvedValue({
      ok: true,
      cgiOk: true,
      webOk: true,
      urls: ['http://10.0.0.5/cgi-bin/server.cgi?action=ls&path=/', 'http://10.0.0.5/index.html']
    })
    ;(window as any).app.openExternal = jest.fn().mockResolvedValue({ ok: false, error: 'blocked' })

    render(<USBDongle />)

    fireEvent.click(screen.getByText('settings.enableDevTools'))

    await waitFor(() => {
      expect(screen.getByText('settings.tryOneOfUrls')).toBeInTheDocument()
    })

    expect(screen.getByText('http://10.0.0.5/index.html')).toBeInTheDocument()
  })

  test('cleans up dev tools state when dongle disconnects', async () => {
    const { rerender } = render(<USBDongle />)

    fireEvent.click(screen.getByText('settings.enableDevTools'))

    await waitFor(() => {
      expect((window as any).projection.usb.uploadLiviScripts).toHaveBeenCalled()
    })

    state.isDongleConnected = false
    rerender(<USBDongle />)

    expect(screen.queryByText('settings.devToolsEnabled')).not.toBeInTheDocument()
  })
  test('auto closes firmware dialog after download is ready with saved message', async () => {
    jest.useFakeTimers()

    render(<USBDongle />)

    act(() => {
      onEventCb?.(null, { type: 'fwUpdate', stage: 'download:start' })
      onEventCb?.(null, {
        type: 'fwUpdate',
        stage: 'download:done',
        path: '/tmp/fw.bin'
      })
    })

    expect(screen.getByText('Dongle Firmware')).toBeInTheDocument()
    expect(screen.getByText('Done')).toBeInTheDocument()
    expect(screen.getByText('Saved to: /tmp/fw.bin')).toBeInTheDocument()

    act(() => {
      jest.advanceTimersByTime(900)
    })

    await waitFor(() => {
      expect(screen.queryByText('Dongle Firmware')).not.toBeInTheDocument()
    })

    jest.useRealTimers()
  })

  test('closes firmware dialog after upload finished and dongle disconnects then reconnects', async () => {
    ;(window as any).projection.ipc.dongleFirmware = jest.fn(async (action: string) => {
      if (action === 'status') {
        return {
          ok: true,
          raw: { err: 0, ver: '2025.02.01.0001' },
          request: {
            local: {
              ok: true,
              ready: true,
              path: '/tmp/fw.bin',
              bytes: 4096,
              latestVer: '2025.02.01.0002'
            }
          }
        }
      }

      if (action === 'upload') {
        return {
          ok: true,
          raw: { err: 0, ver: '2025.02.01.0002' },
          request: {
            local: {
              ok: true,
              ready: true,
              path: '/tmp/fw.bin',
              bytes: 4096,
              latestVer: '2025.02.01.0002'
            }
          }
        }
      }

      return {
        ok: true,
        raw: { err: 0, ver: '-' },
        request: {
          local: {
            ok: true,
            ready: true,
            path: '/tmp/fw.bin',
            bytes: 4096,
            latestVer: '2025.02.01.0002'
          }
        }
      }
    })

    const { rerender } = render(<USBDongle />)

    await waitFor(() => {
      expect((window as any).projection.ipc.dongleFirmware).toHaveBeenCalledWith('status')
    })

    const uploadBtn = await screen.findByText('Upload')
    expect(uploadBtn).not.toBeDisabled()

    fireEvent.click(uploadBtn)

    await waitFor(() => {
      expect((window as any).projection.ipc.dongleFirmware).toHaveBeenCalledWith('upload')
    })

    act(() => {
      onEventCb?.(null, {
        type: 'fwUpdate',
        stage: 'upload:done',
        message: 'Upload complete'
      })
    })

    expect(screen.getByText('Dongle Firmware')).toBeInTheDocument()
    expect(screen.getByText('Upload complete')).toBeInTheDocument()

    state.isDongleConnected = false
    rerender(<USBDongle />)

    state.isDongleConnected = true
    rerender(<USBDongle />)

    await waitFor(() => {
      expect(screen.queryByText('Dongle Firmware')).not.toBeInTheDocument()
    })
  })
})
