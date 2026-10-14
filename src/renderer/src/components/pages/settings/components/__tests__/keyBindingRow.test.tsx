import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { KeyBindingRow } from '../keyBindingRow'

const mockSaveSettings = vi.fn()
let mockSettings: any = null

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      saveSettings: mockSaveSettings,
      settings: mockSettings
    })
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (x: string) => `t:${x}` })
}))

vi.mock('../stackItem', () => ({
  StackItem: ({ children, onClick }: { children: React.ReactNode; onClick: () => void }) => (
    <div role="button" data-testid="stack-item" onClick={onClick}>
      {children}
    </div>
  )
}))

describe('KeyBindingRow', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    mockSettings = {
      bindings: { next: 'MediaNextTrack' }
    }
  })

  afterEach(() => {
    Reflect.deleteProperty(navigator, 'keyboard')
  })

  const setLayout = (entries: Array<[string, string]>) => {
    Object.defineProperty(navigator, 'keyboard', {
      configurable: true,
      value: { getLayoutMap: async () => new Map(entries) }
    })
  }

  const node = {
    kind: 'keyBinding',
    id: 'kb.next',
    label: 'Next',
    labelKey: 'settings.key.next',
    bindingKey: 'next',
    defaultValue: 'ArrowRight'
  } as any

  test('captures and saves a non-modifier key', async () => {
    render(<KeyBindingRow node={node} />)

    fireEvent.click(screen.getByTestId('stack-item'))

    expect(screen.getByText(/Press a key for/)).toBeInTheDocument()

    fireEvent.keyDown(document, { key: 'Shift', code: 'ShiftLeft' })
    expect(mockSaveSettings).not.toHaveBeenCalled()

    fireEvent.keyDown(document, { key: 'x', code: 'KeyX' })

    await waitFor(() => {
      expect(mockSaveSettings).toHaveBeenCalledWith({
        ...mockSettings,
        bindings: {
          ...mockSettings.bindings,
          next: 'KeyX'
        }
      })
    })
  })

  test('esc cancels capture and backspace binds as a normal key', async () => {
    render(<KeyBindingRow node={node} />)

    fireEvent.click(screen.getByTestId('stack-item'))
    fireEvent.keyDown(document, { key: 'Escape', code: 'Escape' })

    await waitFor(() => {
      expect(screen.queryByText(/Press a key for/)).not.toBeInTheDocument()
    })

    fireEvent.click(screen.getByTestId('stack-item'))
    fireEvent.keyDown(document, { key: 'Backspace', code: 'Backspace' })

    await waitFor(() => {
      expect(mockSaveSettings).toHaveBeenCalledWith({
        ...mockSettings,
        bindings: {
          ...mockSettings.bindings,
          next: 'Backspace'
        }
      })
    })
  })

  test('unbind icon click clears the binding', async () => {
    render(<KeyBindingRow node={node} />)

    const buttons = screen.getAllByRole('button')
    fireEvent.click(buttons[1] as HTMLElement)

    await waitFor(() => {
      expect(mockSaveSettings).toHaveBeenCalledWith({
        ...mockSettings,
        bindings: {
          ...mockSettings.bindings,
          next: ''
        }
      })
    })
  })

  test('reset icon click applies default', async () => {
    render(<KeyBindingRow node={node} />)

    const buttons = screen.getAllByRole('button')
    fireEvent.click(buttons[2] as HTMLElement)

    await waitFor(() => {
      expect(mockSaveSettings).toHaveBeenCalled()
    })
  })

  test('resolves a single-character key through the layout map', async () => {
    setLayout([['MediaNextTrack', 'x']])
    render(<KeyBindingRow node={node} />)

    expect(await screen.findByText('X')).toBeInTheDocument()
  })

  test('resolves a multi-character key through the layout map', async () => {
    setLayout([['MediaNextTrack', 'Enter']])
    render(<KeyBindingRow node={node} />)

    expect(await screen.findByText('Enter')).toBeInTheDocument()
  })

  test('ignores a failing layout map lookup', async () => {
    Object.defineProperty(navigator, 'keyboard', {
      configurable: true,
      value: { getLayoutMap: async () => Promise.reject(new Error('no layout')) }
    })
    render(<KeyBindingRow node={node} />)

    await waitFor(() => {
      expect(screen.getByText('MediaNextTrack')).toBeInTheDocument()
    })
  })

  test('shows placeholder when no binding is set', () => {
    mockSettings = { bindings: {} }
    render(<KeyBindingRow node={node} />)

    expect(screen.getByText('---')).toBeInTheDocument()
  })

  test('does not save when settings are missing', () => {
    mockSettings = null
    render(<KeyBindingRow node={node} />)

    fireEvent.click(screen.getByTestId('stack-item'))
    fireEvent.keyDown(document, { key: 'y', code: 'KeyY' })

    expect(mockSaveSettings).not.toHaveBeenCalled()
  })

  test('creates a bindings map when settings have none', async () => {
    mockSettings = {}
    render(<KeyBindingRow node={node} />)

    fireEvent.click(screen.getByTestId('stack-item'))
    fireEvent.keyDown(document, { key: 'y', code: 'KeyY' })

    await waitFor(() => {
      expect(mockSaveSettings).toHaveBeenCalledWith({ bindings: { next: 'KeyY' } })
    })
  })

  test('falls back to the default bindings when node omits a default value', () => {
    const noDefault = {
      kind: 'keyBinding',
      id: 'kb.next',
      label: 'Next',
      labelKey: 'settings.key.next',
      bindingKey: 'next'
    } as any
    mockSettings = { bindings: { next: 'KeyN' } }
    render(<KeyBindingRow node={noDefault} />)

    const buttons = screen.getAllByRole('button')
    expect(buttons[2]).toBeDisabled()
  })

  test('treats an unknown binding key as having no default', () => {
    const bare = {
      kind: 'keyBinding',
      id: 'kb.bare',
      label: 'Bare',
      bindingKey: 'nonexistent_key'
    } as any
    mockSettings = { bindings: {} }
    render(<KeyBindingRow node={bare} />)

    expect(screen.getByText('Bare')).toBeInTheDocument()
    const buttons = screen.getAllByRole('button')
    expect(buttons[2]).toBeDisabled()
  })

  test('closing the modal backdrop cancels capture', () => {
    const { baseElement } = render(<KeyBindingRow node={node} />)

    fireEvent.click(screen.getByTestId('stack-item'))
    expect(screen.getByText(/Press a key for/)).toBeInTheDocument()

    const backdrop = baseElement.querySelector('.MuiBackdrop-root') as HTMLElement
    fireEvent.click(backdrop)

    expect(screen.queryByText(/Press a key for/)).not.toBeInTheDocument()
  })
})
