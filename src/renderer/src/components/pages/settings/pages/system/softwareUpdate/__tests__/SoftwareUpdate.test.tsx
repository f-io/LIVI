import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { SoftwareUpdate } from '../SoftwareUpdate'

let updateEventCb: ((e: any) => void) | undefined
let progressCb: ((p: any) => void) | undefined

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k })
}))

describe('SoftwareUpdate', () => {
  beforeEach(() => {
    updateEventCb = undefined
    progressCb = undefined
    ;(window as any).app = {
      getVersion: jest.fn().mockResolvedValue('1.0.0'),
      getLatestRelease: jest.fn().mockResolvedValue({ version: '1.1.0', url: 'https://u' }),
      performUpdate: jest.fn(),
      onUpdateEvent: jest.fn((cb: any) => {
        updateEventCb = cb
        return () => {}
      }),
      onUpdateProgress: jest.fn((cb: any) => {
        progressCb = cb
        return () => {}
      }),
      abortUpdate: jest.fn(),
      beginInstall: jest.fn()
    }
  })

  test('loads versions and triggers update action', async () => {
    render(<SoftwareUpdate />)

    await waitFor(() => {
      expect(screen.getByText('1.0.0')).toBeInTheDocument()
      expect(screen.getByText('1.1.0')).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole('button', { name: 'Update' }))
    expect((window as any).app.performUpdate).toHaveBeenCalledWith('https://u')
  })

  test('renders progress and ready/install actions from update events', async () => {
    render(<SoftwareUpdate />)

    act(() => {
      progressCb?.({ percent: 0.5, received: 1024, total: 2048 })
    })
    expect(screen.getAllByRole('progressbar').length).toBeGreaterThan(0)

    act(() => {
      updateEventCb?.({ phase: 'ready', message: '' })
    })

    fireEvent.click(screen.getByText('softwareUpdate.installNow'))
    expect((window as any).app.beginInstall).toHaveBeenCalled()
  })

  test('error event shows error message and close button closes dialog', async () => {
    // lines 98-100: error phase sets error state; line 250: close button
    render(<SoftwareUpdate />)

    // Open the dialog first via ready event (triggers upDialogOpen = true)
    act(() => {
      updateEventCb?.({ phase: 'ready', message: '' })
    })

    // Now fire the error event — dialog stays open, error message rendered
    act(() => {
      updateEventCb?.({ phase: 'error', message: 'network timeout' })
    })

    expect(screen.getByText('network timeout')).toBeInTheDocument()
    const closeBtn = screen.getByText('softwareUpdate.close')
    fireEvent.click(closeBtn)
    // dialog should be gone
    expect(screen.queryByText('softwareUpdate.close')).not.toBeInTheDocument()
  })

  test('aborted error phase auto-closes dialog after 1200ms', async () => {
    // lines 87-92: phase=error + /aborted/ → setTimeout(handleCloseAndReset, 1200)
    jest.useFakeTimers()
    render(<SoftwareUpdate />)

    act(() => {
      updateEventCb?.({ phase: 'ready', message: '' })
    })
    // dialog is open now (ready phase)
    expect(screen.getByText('softwareUpdate.installNow')).toBeInTheDocument()

    act(() => {
      updateEventCb?.({ phase: 'error', message: 'Download aborted' })
    })

    // not yet closed
    act(() => {
      jest.advanceTimersByTime(1199)
    })
    // still visible
    expect(screen.getByText('softwareUpdate.close')).toBeInTheDocument()

    act(() => {
      jest.advanceTimersByTime(2)
    })
    // auto-closed
    expect(screen.queryByText('softwareUpdate.close')).not.toBeInTheDocument()

    jest.useRealTimers()
  })

  test('handlePrimaryAction when inFlight opens the dialog', async () => {
    // lines 135-137: inFlight → setUpDialogOpen(true)
    render(<SoftwareUpdate />)

    // trigger in-flight state via progress event
    act(() => {
      progressCb?.({ percent: 0.2, received: 200, total: 1000 })
    })

    await waitFor(() => expect(screen.getByText('1.0.0')).toBeInTheDocument())

    // The main button should now be the "update" button but disabled
    // Clicking the "Check" button (recheck) while in-flight is disabled
    // Click the primary button while inFlight → should open dialog
    const primaryBtn = screen.getByRole('button', { name: 'Update' })
    fireEvent.click(primaryBtn)

    // Dialog should be open (indeterminate progress visible)
    expect(screen.getAllByRole('progressbar').length).toBeGreaterThan(0)
  })

  test('getLatestRelease failure shows error message', async () => {
    // lines 70-73: catch → setLatestVersion(''), setMessage(t(...couldNotCheck...))
    ;(window as any).app.getLatestRelease = jest.fn().mockRejectedValue(new Error('network fail'))
    render(<SoftwareUpdate />)

    await waitFor(() => {
      expect(screen.getByText('softwareUpdate.couldNotCheckLatestRelease')).toBeInTheDocument()
    })
  })

  test('getLatestRelease returning no version shows message', async () => {
    // line 66: r.version falsy → setMessage(t(...couldNotCheck...))
    ;(window as any).app.getLatestRelease = jest
      .fn()
      .mockResolvedValue({ version: null, url: null })
    render(<SoftwareUpdate />)

    await waitFor(() => {
      expect(screen.getByText('softwareUpdate.couldNotCheckLatestRelease')).toBeInTheDocument()
    })
  })
})
