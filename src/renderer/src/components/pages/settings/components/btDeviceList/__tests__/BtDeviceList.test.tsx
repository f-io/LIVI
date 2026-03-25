import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { BtDeviceList } from '../BtDeviceList'

const mockUseLiviStore = jest.fn()
const removeMock = jest.fn()

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
  })

  const renderWithState = (state: {
    bluetoothPairedDevices: Array<{ mac: string; name: string }> | unknown
    removeBluetoothPairedDeviceLocal: (mac: string) => void
    boxInfo?: {
      btMacAddr?: string
      DevList?: Array<{ id: string; type?: string; index?: number | string }>
    }
  }) => {
    mockUseLiviStore.mockImplementation((selector) => selector(state))
    return render(React.createElement(BtDeviceList))
  }

  test('renders empty list when devices is not an array', () => {
    renderWithState({
      bluetoothPairedDevices: null,
      removeBluetoothPairedDeviceLocal: removeMock,
      boxInfo: undefined
    })

    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  test('renders devices sorted by connected device first, then by index', () => {
    renderWithState({
      bluetoothPairedDevices: [
        { mac: 'BB:BB:BB:BB:BB:BB', name: 'Device B' },
        { mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' },
        { mac: 'CC:CC:CC:CC:CC:CC', name: 'Device C' }
      ],
      removeBluetoothPairedDeviceLocal: removeMock,
      boxInfo: {
        btMacAddr: 'CC:CC:CC:CC:CC:CC',
        DevList: [
          { id: 'AA:AA:AA:AA:AA:AA', type: 'Phone', index: 2 },
          { id: 'BB:BB:BB:BB:BB:BB', type: 'Tablet', index: 1 },
          { id: 'CC:CC:CC:CC:CC:CC', type: 'Laptop', index: 3 }
        ]
      }
    })

    const labels = screen.getAllByText(/ - /).map((el) => el.textContent)
    expect(labels).toEqual(['Device C - Laptop', 'Device B - Tablet', 'Device A - Phone'])
  })

  test('falls back to unknown device and unknown type', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: '   ' }],
      removeBluetoothPairedDeviceLocal: removeMock,
      boxInfo: {
        btMacAddr: '',
        DevList: []
      }
    })

    expect(screen.getByText('Unknown device - Unknown')).toBeInTheDocument()
  })

  test('uses trim on connected mac from boxInfo', () => {
    renderWithState({
      bluetoothPairedDevices: [
        { mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' },
        { mac: 'BB:BB:BB:BB:BB:BB', name: 'Device B' }
      ],
      removeBluetoothPairedDeviceLocal: removeMock,
      boxInfo: {
        btMacAddr: '  BB:BB:BB:BB:BB:BB  ',
        DevList: [
          { id: 'AA:AA:AA:AA:AA:AA', type: 'Phone', index: 1 },
          { id: 'BB:BB:BB:BB:BB:BB', type: 'Tablet', index: 2 }
        ]
      }
    })

    const labels = screen.getAllByText(/ - /).map((el) => el.textContent)
    expect(labels[0]).toBe('Device B - Tablet')
  })

  test('calls remove handler with device mac on click', () => {
    renderWithState({
      bluetoothPairedDevices: [{ mac: 'AA:AA:AA:AA:AA:AA', name: 'Device A' }],
      removeBluetoothPairedDeviceLocal: removeMock,
      boxInfo: {
        btMacAddr: '',
        DevList: [{ id: 'AA:AA:AA:AA:AA:AA', type: 'Phone', index: 1 }]
      }
    })

    fireEvent.click(screen.getByRole('button'))

    expect(removeMock).toHaveBeenCalledTimes(1)
    expect(removeMock).toHaveBeenCalledWith('AA:AA:AA:AA:AA:AA')
  })
})
