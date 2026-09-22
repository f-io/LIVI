import { fireEvent, render, screen } from '@testing-library/react'
import { SettingsPage } from '../SettingsPage'

const navigateMock = vi.fn()
let mockNode: any = null
let mockSplat: string | undefined = 'audio'
const handleFieldChange = vi.fn()
const restartMock = vi.fn()
const applyBtList = vi.fn()

const statusState = {
  isDongleHardwarePresent: true,
  activeProtocol: null as 'carplay' | 'androidauto' | null
}
const liviState = {
  settings: { some: 'settings', wirelessAaEnabled: false } as Record<string, unknown>,
  bluetoothPairedDirty: false,
  applyBluetoothPairedList: applyBtList
}
const smartState = {
  state: { audio: { mute: false } } as unknown,
  handleFieldChange,
  needsRestart: false as boolean,
  restart: restartMock,
  requestRestart: vi.fn()
}

vi.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useParams: () => ({ '*': mockSplat })
}))

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string, fb?: string) => fb ?? k })
}))

vi.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) => selector(statusState),
  useLiviStore: (selector: (s: any) => unknown) => selector(liviState),
  useProjectionActive: () =>
    statusState.isDongleHardwarePresent || statusState.activeProtocol !== null
}))

vi.mock('../hooks/useSmartSettingsFromSchema', () => ({
  useSmartSettingsFromSchema: () => smartState
}))

vi.mock('../utils', () => ({
  getNodeByPath: () => mockNode,
  getValueByPath: (_s: any, _p: string) => false
}))

vi.mock('../components', () => ({
  StackItem: ({ children, onClick }: any) => (
    <button data-testid="stack-item" onClick={onClick}>
      {children}
    </button>
  ),
  KeyBindingRow: () => <div data-testid="keybinding-row" />
}))

vi.mock('../components/SettingsFieldPage', () => ({
  SettingsFieldPage: ({
    onChange,
    onLabelChange,
    onDone
  }: {
    onChange: (v: unknown) => void
    onLabelChange?: (label: string) => void
    onDone: () => void
  }) => (
    <div data-testid="field-page">
      <button data-testid="field-page-change" onClick={() => onChange('next-page-value')} />
      <button
        data-testid="field-page-label"
        onClick={() => onLabelChange?.('page-label')}
        disabled={!onLabelChange}
      />
      <button data-testid="field-page-done" onClick={() => onDone()} />
    </div>
  )
}))

vi.mock('../components/SettingsFieldRow', () => ({
  SettingsFieldRow: ({
    onChange,
    onClick,
    onItemNavigate,
    onLabelChange
  }: {
    onChange: (v: unknown) => void
    onClick?: () => void
    onItemNavigate: (s: string) => void
    onLabelChange?: (label: string) => void
  }) => (
    <div data-testid="field-row">
      <button data-testid="field-row-change" onClick={() => onChange('next')} />
      <button data-testid="field-row-click" onClick={() => onClick?.()} disabled={!onClick} />
      <button data-testid="field-row-navigate" onClick={() => onItemNavigate('child')} />
      <button
        data-testid="field-row-label"
        onClick={() => onLabelChange?.('row-label')}
        disabled={!onLabelChange}
      />
    </div>
  )
}))

vi.mock('../../../layouts', () => ({
  SettingsLayout: ({ title, children, onRestart }: any) => (
    <div>
      <h1>{title}</h1>
      <button data-testid="restart" onClick={onRestart} />
      {children}
    </div>
  )
}))

describe('SettingsPage', () => {
  beforeEach(() => {
    mockNode = null
    mockSplat = 'audio'
    navigateMock.mockReset()
    restartMock.mockReset()
    applyBtList.mockReset()
    handleFieldChange.mockReset()
    statusState.isDongleHardwarePresent = true
    statusState.activeProtocol = null
    liviState.settings = { some: 'settings', wirelessAaEnabled: false }
    liviState.bluetoothPairedDirty = false
    smartState.needsRestart = false
  })

  test('returns null when node is not found', () => {
    const { container } = render(<SettingsPage />)
    expect(container.firstChild).toBeNull()
  })

  test('renders field page for nodes with page metadata', () => {
    mockNode = { type: 'string', label: 'Name', path: 'name', page: { title: 'Name' } }
    render(<SettingsPage />)
    expect(screen.getByText('Name')).toBeInTheDocument()
    expect(screen.getByTestId('field-page')).toBeInTheDocument()
  })

  test('renders mixed route children and handles route click', () => {
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [
        { type: 'route', route: 'advanced', label: 'Advanced', path: '' },
        {
          type: 'custom',
          label: 'Custom',
          path: 'x',
          component: () => <div data-testid="custom" />
        },
        { type: 'keybinding', label: 'Up', path: 'bindings', bindingKey: 'up' },
        { type: 'checkbox', label: 'Mute', path: 'mute' }
      ]
    }

    render(<SettingsPage />)
    expect(screen.getByTestId('stack-item')).toBeInTheDocument()
    expect(screen.getByTestId('custom')).toBeInTheDocument()
    expect(screen.getByTestId('keybinding-row')).toBeInTheDocument()
    expect(screen.getByTestId('field-row')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('stack-item'))
    expect(navigateMock).toHaveBeenCalledWith('advanced')
  })

  test('hidden route children are not rendered', () => {
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [
        { type: 'route', route: 'gone', label: 'Hidden', path: '', hidden: true },
        { type: 'route', route: 'shown', label: 'Shown', path: '' }
      ]
    }
    render(<SettingsPage />)
    const items = screen.getAllByTestId('stack-item')
    expect(items).toHaveLength(1)
    expect(items[0]).toHaveTextContent('Shown')
  })

  test('page field passes onChange through to handleFieldChange', () => {
    mockNode = { type: 'string', label: 'Name', path: 'name', page: { title: 'Name' } }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('field-page-change'))
    expect(handleFieldChange).toHaveBeenCalledWith('name', 'next-page-value')
  })

  test('field row click navigates to the child path when the child has a page', () => {
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [{ type: 'string', label: 'Field', path: 'field', page: { title: 'X' } }]
    }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('field-row-change'))
    expect(handleFieldChange).toHaveBeenCalledWith('field', 'next')

    fireEvent.click(screen.getByTestId('field-row-click'))
    expect(navigateMock).toHaveBeenCalledWith('field')

    fireEvent.click(screen.getByTestId('field-row-navigate'))
    expect(navigateMock).toHaveBeenCalledWith('child')
  })

  test('custom child forwards onChange to handleFieldChange', () => {
    let captured: ((v: unknown) => void) | null = null
    const CustomCmp = (props: { onChange: (v: unknown) => void }) => {
      captured = props.onChange
      return <div data-testid="custom" />
    }
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [{ type: 'custom', label: 'Custom', path: 'cx', component: CustomCmp }]
    }
    render(<SettingsPage />)
    captured!('changed')
    expect(handleFieldChange).toHaveBeenCalledWith('cx', 'changed')
  })

  test('handleRestart no-ops when neither dongle nor AA is connected', () => {
    statusState.isDongleHardwarePresent = false
    statusState.activeProtocol = null
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(restartMock).not.toHaveBeenCalled()
    expect(applyBtList).not.toHaveBeenCalled()
  })

  test('handleRestart no-ops when restart is available but nothing is pending', () => {
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(restartMock).not.toHaveBeenCalled()
    expect(applyBtList).not.toHaveBeenCalled()
  })

  test('handleRestart calls restart() when needsRestart is true', () => {
    smartState.needsRestart = true
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(restartMock).toHaveBeenCalled()
    expect(applyBtList).not.toHaveBeenCalled()
  })

  test('handleRestart applies the BT list when dirty and no restart is pending', () => {
    liviState.bluetoothPairedDirty = true
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(applyBtList).toHaveBeenCalled()
    expect(restartMock).not.toHaveBeenCalled()
  })

  test('AA-active alone is enough to enable restart', () => {
    statusState.isDongleHardwarePresent = false
    liviState.settings = { wirelessAaEnabled: true }
    smartState.needsRestart = true
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(restartMock).toHaveBeenCalled()
  })

  test('select page node wires up the label path and done handler', () => {
    mockNode = {
      type: 'select',
      label: 'Language',
      path: 'lang',
      labelPath: 'lang.label',
      page: { title: 'Language' }
    }
    render(<SettingsPage />)

    fireEvent.click(screen.getByTestId('field-page-label'))
    expect(handleFieldChange).toHaveBeenCalledWith('lang.label', 'page-label')

    fireEvent.click(screen.getByTestId('field-page-done'))
    expect(navigateMock).toHaveBeenCalledWith(-1)
  })

  test('slider page node centers its content', () => {
    mockNode = { type: 'slider', label: 'Volume', path: 'volume', page: { title: 'Volume' } }
    render(<SettingsPage />)
    expect(screen.getByTestId('field-page')).toBeInTheDocument()
    expect(screen.getByTestId('field-page-label')).toBeDisabled()
  })

  test('title falls back to the translated label key', () => {
    mockNode = { type: 'route', labelKey: 'settings.title', children: [] }
    render(<SettingsPage />)
    expect(screen.getByText('settings.title')).toBeInTheDocument()
  })

  test('select child wires up the child label path', () => {
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [{ type: 'select', label: 'Source', path: 'source', labelPath: 'source.label' }]
    }
    render(<SettingsPage />)

    fireEvent.click(screen.getByTestId('field-row-label'))
    expect(handleFieldChange).toHaveBeenCalledWith('source.label', 'row-label')
  })

  test('route child renders its translated label key', () => {
    mockNode = {
      type: 'route',
      label: 'Audio',
      children: [{ type: 'route', route: 'deep', labelKey: 'deep.key', path: '' }]
    }
    render(<SettingsPage />)
    expect(screen.getByTestId('stack-item')).toHaveTextContent('deep.key')
  })

  test('route node with an explicit undefined children list renders no rows', () => {
    mockNode = { type: 'route', label: 'Empty', children: undefined }
    render(<SettingsPage />)
    expect(screen.getByText('Empty')).toBeInTheDocument()
    expect(screen.queryByTestId('field-row')).not.toBeInTheDocument()
  })

  test('node without a children key renders no rows', () => {
    mockNode = { type: 'route', label: 'NoChildren' }
    render(<SettingsPage />)
    expect(screen.getByText('NoChildren')).toBeInTheDocument()
    expect(screen.queryByTestId('field-row')).not.toBeInTheDocument()
  })

  test('empty splat resolves to the root path', () => {
    mockSplat = undefined
    mockNode = { type: 'route', label: 'Root', children: [] }
    render(<SettingsPage />)
    expect(screen.getByText('Root')).toBeInTheDocument()
  })
})
