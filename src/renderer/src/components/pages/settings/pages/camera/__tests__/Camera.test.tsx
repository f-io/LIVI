import { render, screen, waitFor } from '@testing-library/react'
import { Camera } from '../Camera'

const listenForEvents = jest.fn()
const unlistenForEvents = jest.fn()
const setCameraFound = jest.fn()

const detectCameras = jest.fn().mockResolvedValue([
  { deviceId: 'cam-1', label: 'Front cam' },
  { deviceId: 'cam-2', label: 'Rear cam' }
])

jest.mock('@utils/cameraDetection', () => ({
  updateCameras: (...args: unknown[]) => detectCameras(...args)
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) => selector({ setCameraFound })
}))

describe('Settings Camera page', () => {
  beforeEach(() => {
    detectCameras.mockClear()
    setCameraFound.mockClear()
    ;(window as any).projection = {
      usb: {
        listenForEvents,
        unlistenForEvents
      }
    }
    listenForEvents.mockClear()
    unlistenForEvents.mockClear()
  })

  test('loads camera options and subscribes to usb events', async () => {
    const onChange = jest.fn()
    const { unmount } = render(<Camera state={{ camera: '' } as any} onChange={onChange} />)

    await waitFor(() => {
      expect(detectCameras).toHaveBeenCalled()
    })

    expect(screen.getByText('Source')).toBeInTheDocument()
    expect(listenForEvents).toHaveBeenCalled()
    unmount()
    expect(unlistenForEvents).toHaveBeenCalled()
  })
})
