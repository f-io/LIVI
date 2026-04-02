import { PhoneWorkMode } from '@shared/types'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import React from 'react'
import { BtDeviceList } from '../BtDeviceList'

const mockUseLiviStore = jest.fn()
const removeMock = jest.fn()
const connectMock = jest.fn()
const saveSettingsMock = jest.fn()

jest.mock('@renderer/store/store', () => ({
  useLiviStore: (selector: (state: Record<string, unknown>) => unknown) =>
    mockUseLiviStore(selector)
}))

jest.mock('../../stackItem', () => ({
  StackItem: ({ children }: { children: React.ReactNode }) =>
    React.createElement('div', null, children)
}))

describe('BtDeviceList', () => {
  beforeEach(() => {
    jest.clearAllMocks()

    Object.defineProperty(window, 'projection', {
      value: {
        usb: {
          forceReset: jest.fn().mockResolvedValue(undefined)
        }
      },
      writable: true,
      configurable: true
    })
  })

  const renderWithState = (state: {
    bluetoothPairedDevices: Array<{ mac: string; name: string }> | unknown
    //forgetBluetoothPairedDevice?: (mac: string) => void
    removeBluetoothPairedDeviceLocal: (mac: string) => void
    connectBluetoothPairedDevice: (mac: string) => Promise<boolean> | boolean
    saveSettings: (settings: unknown) => Promise<void> | void
    boxInfo?: {
      btMacAddr?: string
      DevList?: Array<{ id: string; type?: string; index?: number | string }>
    }
  }) => {
    mockUseLiviStore.mockImplementation((selector) => selector(state))
    return render(React.createElement(BtDeviceList))
  }

  test('connect click saves PhoneWorkMode.Android for AndroidAuto devices and resets usb on success', async () => {
    connectMock.mockResolvedValue(true)
    saveSettingsMock.mockResolvedValue(undefined)
    const forceResetMock = jest.fn().mockResolvedValue(undefined)
    window.projection.usb.forceReset = forceResetMock

    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Android Device' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
      }
    })

    fireEvent.click(screen.getByTestId('LinkIcon').closest('button')!)

    await waitFor(() => {
      expect(saveSettingsMock).toHaveBeenCalledWith({
        lastPhoneWorkMode: PhoneWorkMode.Android
      })
    })

    await waitFor(() => {
      expect(connectMock).toHaveBeenCalledWith('AA:AA:AA:AA:AA:AA')
    })

    await waitFor(() => {
      expect(forceResetMock).toHaveBeenCalledTimes(1)
    })
  })

  test('connect click saves PhoneWorkMode.CarPlay for non-AndroidAuto devices', async () => {
    connectMock.mockResolvedValue(true)
    saveSettingsMock.mockResolvedValue(undefined)
    window.projection.usb.forceReset = jest.fn().mockResolvedValue(undefined)

    renderWithState({
      bluetoothPairedDevices: [{ mac: 'BB:BB:BB:BB:BB:BB', name: 'iPhone' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'BB:BB:BB:BB:BB:BB', type: 'CarPlay', index: 1 }]
      }
    })

    fireEvent.click(screen.getByTestId('LinkIcon').closest('button')!)

    await waitFor(() => {
      expect(saveSettingsMock).toHaveBeenCalledWith({
        lastPhoneWorkMode: PhoneWorkMode.CarPlay
      })
    })
  })

  test('clears pending state and does not reset usb when connect fails', async () => {
    connectMock.mockResolvedValue(false)
    saveSettingsMock.mockResolvedValue(undefined)
    const forceResetMock = jest.fn().mockResolvedValue(undefined)
    window.projection.usb.forceReset = forceResetMock

    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
      }
    })

    const button = screen.getByTestId('LinkIcon').closest('button')!
    fireEvent.click(button)

    await waitFor(() => {
      expect(connectMock).toHaveBeenCalledWith('AA:AA:AA:AA:AA:AA')
    })

    expect(forceResetMock).not.toHaveBeenCalled()
  })

  test('clears pending state when usb forceReset fails', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    connectMock.mockResolvedValue(true)
    saveSettingsMock.mockResolvedValue(undefined)
    window.projection.usb.forceReset = jest.fn().mockRejectedValue(new Error('reset failed'))

    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
      }
    })

    fireEvent.click(screen.getByTestId('LinkIcon').closest('button')!)

    await waitFor(() => {
      expect(window.projection.usb.forceReset).toHaveBeenCalledTimes(1)
    })

    await waitFor(() => {
      expect(warnSpy).toHaveBeenCalled()
    })

    warnSpy.mockRestore()
  })

  test('clears pending state once device becomes connected', async () => {
    connectMock.mockResolvedValue(true)
    saveSettingsMock.mockResolvedValue(undefined)

    const state: {
      bluetoothPairedDevices: Array<{ mac: string; name: string }>
      //forgetBluetoothPairedDevice: typeof removeMock
      removeBluetoothPairedDeviceLocal: typeof removeMock
      connectBluetoothPairedDevice: typeof connectMock
      saveSettings: typeof saveSettingsMock
      boxInfo?: {
        btMacAddr?: string
        DevList?: Array<{ id: string; type?: string; index?: number | string }>
      }
    } = {
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
      }
    }

    mockUseLiviStore.mockImplementation((selector) => selector(state))
    const view = render(<BtDeviceList />)

    fireEvent.click(screen.getByTestId('LinkIcon').closest('button')!)

    await waitFor(() => {
      expect(connectMock).toHaveBeenCalled()
    })

    state.boxInfo = {
      btMacAddr: 'AA:AA:AA:AA:AA:AA',
      DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
    }

    view.rerender(<BtDeviceList />)

    await waitFor(() => {
      expect(screen.getByTestId('LinkIcon').closest('button')).toBeDisabled()
    })
  })

  test('calls remove handler with device mac on click', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'CarPlay', index: 1 }]
      }
    })

    fireEvent.click(screen.getByTestId('CloseIcon').closest('button')!)

    expect(removeMock).toHaveBeenCalledTimes(1)
    expect(removeMock).toHaveBeenCalledWith('AA:AA:AA:AA:AA:AA')
  })

  test('handles missing boxInfo gracefully', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: undefined
    })

    expect(screen.getByText('Device A')).toBeInTheDocument()
  })

  test('uses cached device metadata when DevList entry is no longer present', () => {
    let state = {
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 }]
      }
    }

    mockUseLiviStore.mockImplementation((selector) => selector(state))
    const view = render(<BtDeviceList />)

    expect(screen.getByText('Android Auto')).toBeInTheDocument()

    state = {
      ...state,
      boxInfo: {
        btMacAddr: '',
        DevList: []
      }
    }

    view.rerender(<BtDeviceList />)

    expect(screen.getByText('Android Auto')).toBeInTheDocument()
  })

  test('disables buttons while a device switch is pending', async () => {
    let resolveConnect: (value: boolean) => void = () => {}
    connectMock.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          resolveConnect = resolve
        })
    )
    saveSettingsMock.mockResolvedValue(undefined)
    window.projection.usb.forceReset = jest.fn().mockResolvedValue(undefined)

    renderWithState({
      bluetoothPairedDevices: [
        { mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' },
        { mac: 'BB:BB:BB:BB:BB:BB', name: 'Device B' }
      ],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [
          { id: 'AA:AA:AA:AA:AA:AA', type: 'AndroidAuto', index: 1 },
          { id: 'BB:BB:BB:BB:BB:BB', type: 'CarPlay', index: 2 }
        ]
      }
    })

    const linkButtons = screen.getAllByTestId('LinkIcon').map((icon) => icon.closest('button')!)
    fireEvent.click(linkButtons[0])

    await waitFor(() => {
      expect(saveSettingsMock).toHaveBeenCalled()
    })

    expect(linkButtons[0]).toBeDisabled()
    expect(linkButtons[1]).toBeDisabled()

    resolveConnect(true)
  })

  test('renders no buttons when bluetoothPairedDevices is not an array', () => {
    renderWithState({
      bluetoothPairedDevices: null,
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: []
      }
    })

    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  test('ignores DevList entries with empty ids', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [
          { id: '   ', type: 'AndroidAuto', index: 1 },
          { id: 'AA:AA:AA:AA:AA:AA', type: 'CarPlay', index: 2 }
        ]
      }
    })

    expect(screen.getByText('Device A')).toBeInTheDocument()
    expect(screen.getByText('CarPlay')).toBeInTheDocument()
  })

  test('falls back to index 999 when DevList entry has no index', () => {
    renderWithState({
      bluetoothPairedDevices: [
        { mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' },
        { mac: 'BB:BB:BB:BB:BB:BB', name: 'Device B' }
      ],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [
          { id: 'AA:AA:AA:AA:AA:AA', type: 'CarPlay' },
          { id: 'BB:BB:BB:BB:BB:BB', type: 'CarPlay', index: 1 }
        ]
      }
    })

    const names = screen.getAllByText(/Device [AB]/).map((el) => el.textContent?.trim())

    expect(names).toEqual(['Device B', 'Device A'])
  })

  test('falls back to "Unknown device" when device name is blank', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: '   ' }],
      //forgetBluetoothPairedDevice: removeMock,
      removeBluetoothPairedDeviceLocal: removeMock,
      connectBluetoothPairedDevice: connectMock,
      saveSettings: saveSettingsMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'CarPlay', index: 1 }]
      }
    })

    expect(screen.getByText('Unknown device')).toBeInTheDocument()
  })
})
