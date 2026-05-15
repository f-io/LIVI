import { fireEvent, render, screen } from '@testing-library/react'
import { SettingsPage } from '../SettingsPage'

const navigateMock = jest.fn()
let mockNode: any = null
const handleFieldChange = jest.fn()
const restartMock = jest.fn()
const applyBtList = jest.fn()

const statusState = { isDongleConnected: true, isAaActive: false }
const liviState = {
  settings: { some: 'settings', aa: false } as Record<string, unknown>,
  bluetoothPairedDirty: false,
  applyBluetoothPairedList: applyBtList
}
const smartState = {
  state: { audio: { mute: false } } as unknown,
  handleFieldChange,
  needsRestart: false as boolean,
  restart: restartMock,
  requestRestart: jest.fn()
}

jest.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useParams: () => ({ '*': 'audio' })
}))

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string, fb?: string) => fb ?? k })
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) => selector(statusState),
  useLiviStore: (selector: (s: any) => unknown) => selector(liviState)
}))

jest.mock('../hooks/useSmartSettingsFromSchema', () => ({
  useSmartSettingsFromSchema: () => smartState
}))

jest.mock('../utils', () => ({
  getNodeByPath: () => mockNode,
  getValueByPath: (_s: any, _p: string) => false
}))

jest.mock('../components', () => ({
  StackItem: ({ children, onClick }: any) => (
    <button data-testid="stack-item" onClick={onClick}>
      {children}
    </button>
  ),
  KeyBindingRow: () => <div data-testid="keybinding-row" />
}))

jest.mock('../components/SettingsFieldPage', () => ({
  SettingsFieldPage: ({ onChange }: { onChange: (v: unknown) => void }) => (
    <button data-testid="field-page" onClick={() => onChange('next-page-value')} />
  )
}))

jest.mock('../components/SettingsFieldRow', () => ({
  SettingsFieldRow: ({
    onChange,
    onClick,
    onItemNavigate
  }: {
    onChange: (v: unknown) => void
    onClick?: () => void
    onItemNavigate: (s: string) => void
  }) => (
    <div data-testid="field-row">
      <button data-testid="field-row-change" onClick={() => onChange('next')} />
      <button data-testid="field-row-click" onClick={() => onClick?.()} disabled={!onClick} />
      <button data-testid="field-row-navigate" onClick={() => onItemNavigate('child')} />
    </div>
  )
}))

jest.mock('../../../layouts', () => ({
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
    navigateMock.mockReset()
    restartMock.mockReset()
    applyBtList.mockReset()
    handleFieldChange.mockReset()
    statusState.isDongleConnected = true
    statusState.isAaActive = false
    liviState.settings = { some: 'settings', aa: false }
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
    fireEvent.click(screen.getByTestId('field-page'))
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
    statusState.isDongleConnected = false
    statusState.isAaActive = false
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
    statusState.isDongleConnected = false
    liviState.settings = { aa: true }
    smartState.needsRestart = true
    mockNode = { type: 'route', label: 'Audio', children: [] }
    render(<SettingsPage />)
    fireEvent.click(screen.getByTestId('restart'))
    expect(restartMock).toHaveBeenCalled()
  })
})
