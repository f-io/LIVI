import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Debug } from '../Debug'

let onEventCb: ((e: unknown, ...args: unknown[]) => void) | undefined

describe('Debug page', () => {
  beforeEach(() => {
    onEventCb = undefined
    ;(window as any).projection = {
      ipc: {
        readNavigation: jest.fn().mockResolvedValue({ route: 'Main St' }),
        readMedia: jest.fn().mockResolvedValue({ artist: 'Artist' }),
        onEvent: jest.fn((cb: any) => {
          onEventCb = cb
        }),
        offEvent: jest.fn()
      }
    }
  })

  test('loads initial snapshots and logs incoming events', async () => {
    render(<Debug />)

    await waitFor(() => {
      expect((window as any).projection.ipc.readNavigation).toHaveBeenCalled()
      expect((window as any).projection.ipc.readMedia).toHaveBeenCalled()
    })

    expect(screen.getByText(/navigationData\.json/i)).toBeInTheDocument()
    expect(screen.getByText(/mediaData\.json/i)).toBeInTheDocument()

    act(() => {
      onEventCb?.(null, { type: 'navigation', payload: { turn: 'left' } })
    })

    expect(screen.getByText(/"turn": "left"/i)).toBeInTheDocument()
  })

  test('clear button resets live events list', async () => {
    render(<Debug />)

    act(() => {
      onEventCb?.(null, { type: 'custom', payload: { ok: true } })
    })
    expect(screen.queryByText('No events yet.')).toBeNull()

    fireEvent.click(screen.getByText('Clear'))
    expect(screen.getByText('No events yet.')).toBeInTheDocument()
  })
})
