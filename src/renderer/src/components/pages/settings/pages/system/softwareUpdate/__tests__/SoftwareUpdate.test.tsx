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
})
