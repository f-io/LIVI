import { fireEvent, render, screen } from '@testing-library/react'

const state: {
  settings:
    | (Partial<{
        dashboards: Record<string, Partial<Record<'main' | 'cluster' | 'aux', boolean>>>
        media: Partial<Record<'cluster' | 'aux', boolean>>
        camera: Partial<Record<'cluster' | 'aux', boolean>>
        bindings: Record<string, string>
      }> & { dashboards?: unknown })
    | undefined
} = { settings: undefined }

let clusterDashboardActive = false

vi.mock('../../../store/store', () => ({
  sendInput: (input: unknown) => sendInputMock(input),
  reportShown: (screen: string, front: string) => reportShownMock(screen, front),
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) => selector(state),
  useStatusStore: (selector: (s: { clusterDashboardActive: boolean }) => unknown) =>
    selector({ clusterDashboardActive })
}))

vi.mock('../../pages/camera', () => ({
  Camera: () => <div data-testid="camera-page" />
}))
vi.mock('../../pages/cluster/Cluster', () => ({
  Cluster: ({ visible }: { visible: boolean }) => (
    <div data-testid="cluster-page" data-visible={String(visible)} />
  )
}))
vi.mock('../../pages/media', () => ({
  Media: () => <div data-testid="media-page" />
}))
vi.mock('../../pages/telemetry', () => ({
  Telemetry: ({ windowRole }: { windowRole: string }) => (
    <div data-testid="telemetry-page" data-role={windowRole} />
  )
}))

vi.mock('../../layouts/AppLayout', () => ({
  AppLayout: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="app-layout">{children}</div>
  )
}))

const sendInputMock = vi.fn()
const reportShownMock = vi.fn()
const onMediaKeyMock = vi.fn()
const press = (code: string) => ({ kind: 'key', code, down: true })
const release = (code: string) => ({ kind: 'key', code, down: false })

beforeEach(async () => {
  state.settings = undefined
  clusterDashboardActive = false
  sendInputMock.mockReset()
  reportShownMock.mockReset()
  onMediaKeyMock.mockReset().mockReturnValue(() => {})
  ;(window as unknown as { app: unknown }).app = { onMediaKey: onMediaKeyMock }
  vi.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(async () => vi.restoreAllMocks())

async function renderShell(role: 'cluster' | 'aux' = 'cluster', emptyLabel = 'Cluster Window') {
  const { SecondaryAppShell } = await import('../SecondaryAppShell')
  return render(<SecondaryAppShell role={role} emptyLabel={emptyLabel} />)
}

describe('SecondaryAppShell — empty / loading states', async () => {
  test('renders a blank black canvas while settings are still null', async () => {
    state.settings = undefined
    const { container } = await renderShell()
    expect(container.querySelector('[data-testid="app-layout"]')).toBeNull()
  })

  test('renders the empty-label panel when no slot is enabled for the role', async () => {
    state.settings = { dashboards: {}, media: {}, camera: {} }
    await renderShell('cluster', 'Cluster Window')
    expect(screen.getByText('Cluster Window')).toBeInTheDocument()
  })
})

describe('SecondaryAppShell — initial route selection', () => {
  test('a cluster dashboard (dashboard3/dashboard4) renders the cluster overlay and routes to telemetry', async () => {
    state.settings = { dashboards: { dashboard3: { cluster: true } } }
    await renderShell('cluster')
    // Cluster dash is also a dashboard slot, so the initial route is telemetry.
    expect(screen.getByTestId('telemetry-page')).toHaveAttribute('data-role', 'cluster')
    expect(screen.getByTestId('cluster-page')).toBeInTheDocument()
  })

  test('telemetry routes when a non-cluster dashboard slot is set', async () => {
    state.settings = { dashboards: { dashboard1: { cluster: true } } }
    await renderShell('cluster')
    expect(screen.getByTestId('telemetry-page')).toHaveAttribute('data-role', 'cluster')
    expect(screen.queryByTestId('cluster-page')).toBeNull()
  })

  test('media routes when only media is enabled', async () => {
    state.settings = { media: { cluster: true } }
    await renderShell('cluster')
    expect(screen.getByTestId('media-page')).toBeInTheDocument()
  })

  test('camera routes when only camera is enabled', async () => {
    state.settings = { camera: { cluster: true } }
    await renderShell('cluster')
    expect(screen.getByTestId('camera-page')).toBeInTheDocument()
  })

  test('cluster overlay visibility follows clusterDashboardActive', async () => {
    clusterDashboardActive = true
    state.settings = { dashboards: { dashboard4: { aux: true } } }
    await renderShell('aux')
    expect(screen.getByTestId('cluster-page')).toHaveAttribute('data-visible', 'true')
  })

  test('cluster overlay is hidden while the cluster dashboard has not signalled active', async () => {
    clusterDashboardActive = false
    state.settings = { dashboards: { dashboard4: { aux: true } } }
    await renderShell('aux')
    expect(screen.getByTestId('cluster-page')).toHaveAttribute('data-visible', 'false')
  })

  test('aux role ignores slots that belong to the cluster screen', async () => {
    state.settings = { media: { cluster: true } }
    await renderShell('aux', 'Aux Window')
    expect(screen.getByText('Aux Window')).toBeInTheDocument()
  })
})

describe('SecondaryAppShell — media-key bridge', () => {
  test('subscribes to window.app.onMediaKey on mount', async () => {
    state.settings = { media: { cluster: true } }
    await renderShell()
    expect(onMediaKeyMock).toHaveBeenCalled()
  })

  test('an incoming media key dispatches a car-media-key window event', async () => {
    state.settings = { media: { cluster: true } }
    let captured: ((command: string) => void) | null = null
    onMediaKeyMock.mockImplementation((cb: (c: string) => void) => {
      captured = cb
      return () => {}
    })
    await renderShell()
    const listener = vi.fn()
    window.addEventListener('car-media-key', listener as never)
    captured!('playPause')
    expect(listener).toHaveBeenCalled()
    window.removeEventListener('car-media-key', listener as never)
  })

  test('survives missing window.app.onMediaKey', async () => {
    state.settings = { media: { cluster: true } }
    ;(window as { app?: unknown }).app = {}
    await expect(renderShell()).resolves.toBeDefined()
  })
})

describe('SecondaryAppShell — what the screen shows', () => {
  test('tells core when a cluster dashboard is in front', async () => {
    state.settings = { media: { aux: true } }
    clusterDashboardActive = true
    await renderShell('aux', 'Aux Window')
    expect(reportShownMock).toHaveBeenLastCalledWith('aux', 'cluster')
  })

  test('tells core LIVI is in front otherwise', async () => {
    state.settings = { media: { cluster: true } }
    await renderShell()
    expect(reportShownMock).toHaveBeenLastCalledWith('cluster', 'livi')
  })
})

describe('SecondaryAppShell — key bindings reach core', () => {
  test('transport keys go to core', async () => {
    state.settings = {
      media: { cluster: true },
      bindings: { playPause: 'Space', next: 'KeyN' }
    }
    await renderShell()
    fireEvent.keyDown(document, { code: 'Space' })
    expect(sendInputMock).toHaveBeenCalledWith(press('Space'))

    fireEvent.keyDown(document, { code: 'KeyN' })
    expect(sendInputMock).toHaveBeenCalledWith(press('KeyN'))
  })

  test('unmapped key codes are ignored', async () => {
    state.settings = { media: { cluster: true }, bindings: { playPause: 'Space' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyZ' })
    expect(sendInputMock).not.toHaveBeenCalled()
  })

  test('voiceAssistant fires on press and release', async () => {
    state.settings = { media: { cluster: true }, bindings: { voiceAssistant: 'KeyV' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    expect(sendInputMock).toHaveBeenCalledWith(press('KeyV'))

    sendInputMock.mockClear()
    fireEvent.keyUp(document, { code: 'KeyV' })
    expect(sendInputMock).toHaveBeenCalledWith(release('KeyV'))
  })

  test('repeated voiceAssistant keydown is suppressed', async () => {
    state.settings = { media: { cluster: true }, bindings: { voiceAssistant: 'KeyV' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendInputMock.mockClear()
    fireEvent.keyDown(document, { code: 'KeyV', repeat: true })
    expect(sendInputMock).not.toHaveBeenCalled()
  })

  test('PTT auto-releases on window blur', async () => {
    state.settings = { media: { cluster: true }, bindings: { voiceAssistant: 'KeyV' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendInputMock.mockClear()
    window.dispatchEvent(new Event('blur'))
    expect(sendInputMock).toHaveBeenCalledWith(release('KeyV'))
  })

  test('PTT auto-releases on visibility hidden', async () => {
    state.settings = { media: { cluster: true }, bindings: { voiceAssistant: 'KeyV' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendInputMock.mockClear()
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'hidden'
    })
    document.dispatchEvent(new Event('visibilitychange'))
    expect(sendInputMock).toHaveBeenCalledWith(release('KeyV'))
  })

  test('ignores empty and non-string binding codes', async () => {
    state.settings = {
      media: { cluster: true },
      bindings: { playPause: 'Space', left: '', up: 42 } as unknown as Record<string, string>
    }
    await renderShell()

    fireEvent.keyDown(document, { code: 'Space' })
    expect(sendInputMock).toHaveBeenCalledWith(press('Space'))
  })

  test('ignores non-transport key down and up events', async () => {
    state.settings = { media: { cluster: true }, bindings: { back: 'Backspace' } }
    await renderShell()

    fireEvent.keyDown(document, { code: 'Backspace' })
    fireEvent.keyUp(document, { code: 'Backspace' })
    expect(sendInputMock).not.toHaveBeenCalled()
  })

  test('keeps PTT engaged while the window stays visible', async () => {
    state.settings = { media: { cluster: true }, bindings: { voiceAssistant: 'KeyV' } }
    await renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendInputMock.mockClear()

    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'visible'
    })
    document.dispatchEvent(new Event('visibilitychange'))

    expect(sendInputMock).not.toHaveBeenCalledWith(release('KeyV'))
  })
})
