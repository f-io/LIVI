import { fireEvent, render, screen } from '@testing-library/react'
import { ROUTES } from '../../../constants'

const state: {
  settings:
    | (Partial<{
        dashboards: Record<string, Partial<Record<'dash' | 'aux', boolean>>>
        media: Partial<Record<'dash' | 'aux', boolean>>
        cluster: Partial<Record<'dash' | 'aux', boolean>>
        camera: Partial<Record<'dash' | 'aux', boolean>>
        bindings: Record<string, string>
      }> & { dashboards?: unknown })
    | undefined
} = { settings: undefined }

jest.mock('../../../store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) => selector(state)
}))

// Stub the page components so the shell logic is what we exercise.
jest.mock('../../pages/camera', () => ({
  Camera: () => <div data-testid="camera-page" />
}))
jest.mock('../../pages/cluster/Cluster', () => ({
  Cluster: ({ visible }: { visible: boolean }) => (
    <div data-testid="cluster-page" data-visible={String(visible)} />
  )
}))
jest.mock('../../pages/media', () => ({
  Media: ({ forceHydrate }: { forceHydrate?: boolean }) => (
    <div data-testid="media-page" data-hydrate={String(!!forceHydrate)} />
  )
}))
jest.mock('../../pages/telemetry', () => ({
  Telemetry: ({ windowRole }: { windowRole: string }) => (
    <div data-testid="telemetry-page" data-role={windowRole} />
  )
}))

jest.mock('../../layouts/AppLayout', () => ({
  AppLayout: ({ children }: { children: React.ReactNode }) => (
    <div data-testid="app-layout">{children}</div>
  )
}))

const sendCommandMock = jest.fn()
const onMediaKeyMock = jest.fn()

beforeEach(() => {
  state.settings = undefined
  sendCommandMock.mockReset()
  onMediaKeyMock.mockReset().mockReturnValue(() => {})
  ;(window as unknown as { projection: unknown }).projection = {
    ipc: { sendCommand: sendCommandMock }
  }
  ;(window as unknown as { app: unknown }).app = { onMediaKey: onMediaKeyMock }
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

// Force a re-import per describe to capture the freshest mock state if needed.
function renderShell(role: 'dash' | 'aux' = 'dash', emptyLabel = 'Dash Window') {
  // Lazy import so jest.mock() above is in effect
  const { SecondaryAppShell } = require('../SecondaryAppShell')
  return render(<SecondaryAppShell role={role} emptyLabel={emptyLabel} />)
}

describe('SecondaryAppShell — empty / loading states', () => {
  test('renders a blank black canvas while settings are still null', () => {
    state.settings = undefined
    const { container } = renderShell()
    expect(container.querySelector('[data-testid="app-layout"]')).toBeNull()
  })

  test('renders the empty-label panel when no slot is enabled for the role', () => {
    state.settings = { dashboards: {}, media: {}, cluster: {}, camera: {} }
    renderShell('dash', 'Dash Window')
    expect(screen.getByText('Dash Window')).toBeInTheDocument()
  })
})

describe('SecondaryAppShell — initial route selection', () => {
  test('cluster wins when cluster is enabled for the role', () => {
    state.settings = { cluster: { dash: true } }
    renderShell('dash')
    const cluster = screen.getByTestId('cluster-page')
    expect(cluster).toHaveAttribute('data-visible', 'true')
  })

  test('telemetry routes when no cluster but a dashboard slot is set', () => {
    state.settings = { dashboards: { d1: { dash: true } } }
    renderShell('dash')
    expect(screen.getByTestId('telemetry-page')).toHaveAttribute('data-role', 'dash')
  })

  test('media routes when only media is enabled', () => {
    state.settings = { media: { dash: true } }
    renderShell('dash')
    expect(screen.getByTestId('media-page')).toHaveAttribute('data-hydrate', 'true')
  })

  test('camera routes when only camera is enabled', () => {
    state.settings = { camera: { dash: true } }
    renderShell('dash')
    expect(screen.getByTestId('camera-page')).toBeInTheDocument()
  })

  test('cluster overlay is always rendered when cluster is enabled', () => {
    state.settings = { cluster: { aux: true }, media: { aux: true } }
    renderShell('aux')
    // Initial route is CLUSTER, so visible=true
    expect(screen.getByTestId('cluster-page')).toHaveAttribute('data-visible', 'true')
  })

  test('aux role ignores slots that belong to dash', () => {
    state.settings = { media: { dash: true } }
    renderShell('aux', 'Aux Window')
    expect(screen.getByText('Aux Window')).toBeInTheDocument()
  })
})

describe('SecondaryAppShell — media-key bridge', () => {
  test('subscribes to window.app.onMediaKey on mount', () => {
    state.settings = { media: { dash: true } }
    renderShell()
    expect(onMediaKeyMock).toHaveBeenCalled()
  })

  test('an incoming media key dispatches a car-media-key window event', () => {
    state.settings = { media: { dash: true } }
    let captured: ((command: string) => void) | null = null
    onMediaKeyMock.mockImplementation((cb: (c: string) => void) => {
      captured = cb
      return () => {}
    })
    renderShell()
    const listener = jest.fn()
    window.addEventListener('car-media-key', listener as never)
    captured!('playPause')
    expect(listener).toHaveBeenCalled()
    window.removeEventListener('car-media-key', listener as never)
  })

  test('survives missing window.app.onMediaKey', () => {
    state.settings = { media: { dash: true } }
    ;(window as { app?: unknown }).app = {}
    expect(() => renderShell()).not.toThrow()
  })
})

describe('SecondaryAppShell — key bindings dispatch IPC commands', () => {
  test('transport actions send the command through projection.ipc.sendCommand', () => {
    state.settings = {
      media: { dash: true },
      bindings: { playPause: 'Space', next: 'KeyN' }
    }
    renderShell()
    fireEvent.keyDown(document, { code: 'Space' })
    expect(sendCommandMock).toHaveBeenCalledWith('playPause')

    fireEvent.keyDown(document, { code: 'KeyN' })
    expect(sendCommandMock).toHaveBeenCalledWith('next')
  })

  test('unmapped key codes are ignored', () => {
    state.settings = { media: { dash: true }, bindings: { playPause: 'Space' } }
    renderShell()
    fireEvent.keyDown(document, { code: 'KeyZ' })
    expect(sendCommandMock).not.toHaveBeenCalled()
  })

  test('voiceAssistant fires on press and release', () => {
    state.settings = { media: { dash: true }, bindings: { voiceAssistant: 'KeyV' } }
    renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    expect(sendCommandMock).toHaveBeenCalledWith('voiceAssistant')

    sendCommandMock.mockClear()
    fireEvent.keyUp(document, { code: 'KeyV' })
    expect(sendCommandMock).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('repeated voiceAssistant keydown is suppressed', () => {
    state.settings = { media: { dash: true }, bindings: { voiceAssistant: 'KeyV' } }
    renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendCommandMock.mockClear()
    fireEvent.keyDown(document, { code: 'KeyV', repeat: true })
    expect(sendCommandMock).not.toHaveBeenCalled()
  })

  test('PTT auto-releases on window blur', () => {
    state.settings = { media: { dash: true }, bindings: { voiceAssistant: 'KeyV' } }
    renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendCommandMock.mockClear()
    window.dispatchEvent(new Event('blur'))
    expect(sendCommandMock).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('PTT auto-releases on visibility hidden', () => {
    state.settings = { media: { dash: true }, bindings: { voiceAssistant: 'KeyV' } }
    renderShell()
    fireEvent.keyDown(document, { code: 'KeyV' })
    sendCommandMock.mockClear()
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'hidden'
    })
    document.dispatchEvent(new Event('visibilitychange'))
    expect(sendCommandMock).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('sendCommand failure is swallowed', () => {
    sendCommandMock.mockImplementation(() => {
      throw new Error('ipc down')
    })
    state.settings = { media: { dash: true }, bindings: { next: 'KeyN' } }
    renderShell()
    expect(() => fireEvent.keyDown(document, { code: 'KeyN' })).not.toThrow()
  })
})
