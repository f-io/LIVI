import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { Custom } from '../Custom'

let mockSettings: { darkMode?: boolean; customUrl?: string } | undefined = { darkMode: true }

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) =>
    selector({ settings: mockSettings })
}))

describe('pages/custom Custom', () => {
  const customPageUrl = vi.fn()

  beforeEach(() => {
    customPageUrl.mockReset()
    mockSettings = { darkMode: true }
    ;(window as unknown as { app: unknown }).app = { customPageUrl }
  })

  test('frames the local page once main names it', async () => {
    customPageUrl.mockResolvedValue('app://index.html/custom/index.html')
    const { container } = render(<Custom />)

    await waitFor(() => expect(container.querySelector('iframe')).toBeTruthy())
    const frame = container.querySelector('iframe') as HTMLIFrameElement
    expect(frame.getAttribute('src')).toBe('app://index.html/custom/index.html')
    expect(frame.getAttribute('sandbox')).toBeNull()
  })

  test('holds the frame back until its document loaded, on LIVI colours', async () => {
    customPageUrl.mockResolvedValue('http://127.0.0.1:54321/')
    const { container } = render(<Custom />)

    await waitFor(() => expect(container.querySelector('iframe')).toBeTruthy())
    const frame = container.querySelector('iframe') as HTMLIFrameElement
    const root = container.querySelector('#custom-root') as HTMLElement

    expect(frame.style.visibility).toBe('hidden')
    expect(root.style.backgroundColor).not.toBe('')
    expect(frame.style.backgroundColor).toBe(root.style.backgroundColor)

    fireEvent.load(frame)

    await waitFor(() =>
      expect((container.querySelector('iframe') as HTMLIFrameElement).style.visibility).toBe(
        'visible'
      )
    )
  })

  test('hides the frame again when the address changes', async () => {
    customPageUrl.mockResolvedValue('http://127.0.0.1:54321/')
    const { container, rerender } = render(<Custom />)
    await waitFor(() => expect(container.querySelector('iframe')).toBeTruthy())
    fireEvent.load(container.querySelector('iframe') as HTMLIFrameElement)

    customPageUrl.mockResolvedValue('http://127.0.0.1:54322/')
    mockSettings = { darkMode: true, customUrl: 'http://10.0.0.9/' }
    rerender(<Custom />)

    await waitFor(() =>
      expect((container.querySelector('iframe') as HTMLIFrameElement).style.visibility).toBe(
        'hidden'
      )
    )
  })

  test('frames the proxied address when one is configured', async () => {
    customPageUrl.mockResolvedValue('http://127.0.0.1:54321/')
    const { container } = render(<Custom />)

    await waitFor(() => expect(container.querySelector('iframe')).toBeTruthy())
    expect(container.querySelector('iframe')?.getAttribute('src')).toBe('http://127.0.0.1:54321/')
  })

  test('asks again when the configured address changes', async () => {
    customPageUrl.mockResolvedValue('app://index.html/custom/index.html')
    const { rerender } = render(<Custom />)
    await waitFor(() => expect(customPageUrl).toHaveBeenCalledTimes(1))

    mockSettings = { darkMode: true, customUrl: 'http://10.0.0.9/' }
    rerender(<Custom />)

    await waitFor(() => expect(customPageUrl).toHaveBeenCalledTimes(2))
  })

  test('reloads the frame when the theme switches', async () => {
    customPageUrl.mockResolvedValue('app://index.html/custom/index.html')
    const { container, rerender } = render(<Custom />)
    await waitFor(() => expect(container.querySelector('iframe')).toBeTruthy())
    const before = container.querySelector('iframe')

    mockSettings = { darkMode: false }
    rerender(<Custom />)

    expect(container.querySelector('iframe')).not.toBe(before)
  })

  test('says so when there is no page at all', async () => {
    customPageUrl.mockResolvedValue(null)
    render(<Custom />)

    await waitFor(() => expect(screen.getByText(/no page in the custom folder/i)).toBeTruthy())
  })

  test('says so when the request fails', async () => {
    customPageUrl.mockRejectedValue(new Error('refused'))
    render(<Custom />)

    await waitFor(() => expect(screen.getByText(/no page in the custom folder/i)).toBeTruthy())
  })

  test('shows only the icon while the answer is pending', () => {
    customPageUrl.mockReturnValue(new Promise(() => {}))
    const { container } = render(<Custom />)

    expect(container.querySelector('iframe')).toBeNull()
    expect(screen.queryByText(/no page in the custom folder/i)).toBeNull()
    expect(container.querySelector('svg')).toBeTruthy()
  })

  test('an answer landing after unmount is dropped', async () => {
    let settle: (v: unknown) => void = () => {}
    customPageUrl.mockReturnValue(new Promise((r) => (settle = r)))
    const { unmount, container } = render(<Custom />)

    unmount()
    settle('app://index.html/custom/index.html')
    await Promise.resolve()
    expect(container.querySelector('iframe')).toBeNull()
  })

  test('stays on the icon when the bridge is missing', async () => {
    ;(window as unknown as { app: unknown }).app = {}
    const { container } = render(<Custom />)

    await waitFor(() => expect(container.querySelector('svg')).toBeTruthy())
    expect(container.querySelector('iframe')).toBeNull()
  })
})
