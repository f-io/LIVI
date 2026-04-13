import { fireEvent, render, screen, waitFor } from '@testing-library/react'
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

  test('safeCameraPersist skips onChange when camera is already configured', async () => {
    // lines 26-27: if (state.camera && state.camera !== '') return early
    const onChange = jest.fn()
    render(<Camera state={{ camera: 'cam-1' } as any} onChange={onChange} />)

    await waitFor(() => expect(detectCameras).toHaveBeenCalled())

    // detectCameras is called with (setCameraFound, safeCameraPersist, state) → index 1
    const [, persistFn] = detectCameras.mock.calls[0]
    await persistFn('cam-2')
    expect(onChange).not.toHaveBeenCalled()
  })

  test('safeCameraPersist calls onChange when camera is not yet set', async () => {
    // lines 28-29: cameraId && onChange(cameraId)
    const onChange = jest.fn()
    render(<Camera state={{ camera: '' } as any} onChange={onChange} />)

    await waitFor(() => expect(detectCameras).toHaveBeenCalled())

    const [, persistFn] = detectCameras.mock.calls[0]
    await persistFn('cam-1')
    expect(onChange).toHaveBeenCalledWith('cam-1')
  })

  test('safeCameraPersist accepts object with camera property', async () => {
    // line 27: cfgOrId?.camera branch
    const onChange = jest.fn()
    render(<Camera state={{ camera: '' } as any} onChange={onChange} />)

    await waitFor(() => expect(detectCameras).toHaveBeenCalled())

    const [, persistFn] = detectCameras.mock.calls[0]
    await persistFn({ camera: 'cam-1' })
    expect(onChange).toHaveBeenCalledWith('cam-1')
  })

  test('USB attach event triggers camera re-detection', async () => {
    // lines 38-40: usbHandler fires detectCameras again on attach
    render(<Camera state={{ camera: '' } as any} onChange={jest.fn()} />)

    await waitFor(() => expect(listenForEvents).toHaveBeenCalled())

    const usbHandler = listenForEvents.mock.calls[0][0]
    detectCameras.mockClear()
    usbHandler({}, { type: 'attach' })

    await waitFor(() => expect(detectCameras).toHaveBeenCalledTimes(1))
  })

  test('USB event with irrelevant type does not re-detect cameras', async () => {
    // line 39: type not in list → no detectCameras call
    render(<Camera state={{ camera: '' } as any} onChange={jest.fn()} />)

    await waitFor(() => expect(listenForEvents).toHaveBeenCalled())

    const usbHandler = listenForEvents.mock.calls[0][0]
    detectCameras.mockClear()
    usbHandler({}, { type: 'data' })

    expect(detectCameras).not.toHaveBeenCalled()
  })

  test('camera label falls back to "Camera" when label is empty', async () => {
    // line 50: c.label || 'Camera'
    detectCameras.mockResolvedValueOnce([{ deviceId: 'cam-x', label: '' }])
    // set camera to cam-x so the Select shows the selected label
    render(<Camera state={{ camera: 'cam-x' } as any} onChange={jest.fn()} />)

    await waitFor(() => {
      // MUI Select renders the selected option's label in the DOM
      expect(screen.getByText('Camera')).toBeInTheDocument()
    })
  })

  test('shows "No camera" option label when no cameras detected', async () => {
    // cameras.length === 0 → cameraOptions = [{deviceId:'', label:'No camera'}]
    detectCameras.mockResolvedValueOnce([])
    render(<Camera state={{ camera: '' } as any} onChange={jest.fn()} />)

    await waitFor(() => expect(detectCameras).toHaveBeenCalled())

    // Open the Select to make MUI render the options into the DOM
    fireEvent.mouseDown(screen.getByRole('combobox'))
    await waitFor(() => {
      expect(screen.getByRole('option', { name: 'No camera' })).toBeInTheDocument()
    })
  })
})
