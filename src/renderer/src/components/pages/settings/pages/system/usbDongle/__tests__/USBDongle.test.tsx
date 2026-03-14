import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { USBDongle } from '../USBDongle'

let onEventCb: ((e: unknown, p: unknown) => void) | undefined

const state = {
  isDongleConnected: true,
  isStreaming: false,
  settings: { dongleToolsIp: '' },
  saveSettings: jest.fn().mockResolvedValue(undefined),
  vendorId: 0x1234,
  productId: 0xabcd,
  usbFwVersion: '1.0.0',
  dongleFwVersion: '2025.01.01.0001',
  boxInfo: { uuid: 'u1', MFD: 'mfd', productType: 'p1', DevList: [] },
  negotiatedWidth: 1280,
  negotiatedHeight: 720,
  audioCodec: 'aac',
  audioSampleRate: 48000,
  audioChannels: 2,
  audioBitDepth: 16
}

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (k: string) => k
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
  useNetworkStatus: () => ({ online: true, type: 'wifi', effectiveType: '4g' })
}))

describe('USBDongle', () => {
  beforeEach(() => {
    state.saveSettings.mockClear()
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
  })
})
