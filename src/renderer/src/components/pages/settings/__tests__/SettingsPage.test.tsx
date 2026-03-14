import { fireEvent, render, screen } from '@testing-library/react'
import { SettingsPage } from '../SettingsPage'

const navigateMock = jest.fn()
let mockNode: any = null
const handleFieldChange = jest.fn()
const restartMock = jest.fn()
const applyBtList = jest.fn()

jest.mock('react-router', () => ({
  useNavigate: () => navigateMock,
  useParams: () => ({ '*': 'audio' })
}))

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string, fb?: string) => fb ?? k })
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: any) => unknown) => selector({ isDongleConnected: true }),
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: { some: 'settings' },
      bluetoothPairedDirty: false,
      applyBluetoothPairedList: applyBtList
    })
}))

jest.mock('../hooks/useSmartSettingsFromSchema', () => ({
  useSmartSettingsFromSchema: () => ({
    state: { audio: { mute: false } },
    handleFieldChange,
    needsRestart: false,
    restart: restartMock,
    requestRestart: jest.fn()
  })
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
  SettingsFieldPage: () => <div data-testid="field-page" />
}))

jest.mock('../components/SettingsFieldRow', () => ({
  SettingsFieldRow: () => <div data-testid="field-row" />
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
})
