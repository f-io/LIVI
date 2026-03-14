import { fireEvent, render, screen } from '@testing-library/react'
import { SettingsFieldControl } from '../SettingsFieldControl'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => `t:${k}` })
}))

jest.mock('@mui/material', () => {
  const actual = jest.requireActual('@mui/material')
  return {
    ...actual,
    TextField: ({ value, onChange, type = 'text' }: any) => (
      <input data-testid={`textfield-${type}`} type={type} value={value} onChange={onChange} />
    ),
    Switch: ({ checked, onChange }: any) => (
      <input
        data-testid="switch"
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e, e.currentTarget.checked)}
      />
    ),
    Slider: ({ value, onChange }: any) => (
      <input
        data-testid="slider"
        type="range"
        value={value}
        onChange={(e) => onChange(e, Number(e.currentTarget.value))}
      />
    ),
    Select: ({ value, onChange, children }: any) => (
      <select
        data-testid="select"
        value={value}
        onChange={(e) => onChange({ target: { value: e.currentTarget.value } })}
      >
        {children}
      </select>
    ),
    MenuItem: ({ value, children }: any) => <option value={value}>{children}</option>,
    IconButton: ({ onClick, disabled, children }: any) => (
      <button data-testid="icon-button" disabled={disabled} onClick={onClick}>
        {children}
      </button>
    )
  }
})

jest.mock('../numberSpinner/numberSpinner', () => ({
  __esModule: true,
  default: ({ onValueChange }: { onValueChange: (n: number) => void }) => (
    <div>
      <button data-testid="spinner-ok" onClick={() => onValueChange(42.9)} />
      <button data-testid="spinner-bad" onClick={() => onValueChange(Number.NaN)} />
    </div>
  )
}))

describe('SettingsFieldControl', () => {
  test('string node forwards text changes', () => {
    const onChange = jest.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="old"
        onChange={onChange}
      />
    )
    fireEvent.change(screen.getByTestId('textfield-text'), { target: { value: 'new' } })
    expect(onChange).toHaveBeenCalledWith('new')
  })

  test('number node clamps and ignores non-finite values', () => {
    const onChange = jest.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'number', label: 'FPS', path: 'fps', min: 10, max: 30 } as any}
        value={20}
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('spinner-ok'))
    fireEvent.click(screen.getByTestId('spinner-bad'))
    expect(onChange).toHaveBeenCalledTimes(1)
    expect(onChange).toHaveBeenCalledWith(30)
  })

  test('checkbox node forwards boolean changes', () => {
    const onChange = jest.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'checkbox', label: 'Mute', path: 'mute' } as any}
        value={false}
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('switch'))
    expect(onChange).toHaveBeenCalledWith(true)
  })

  test('slider node converts 0..100 to fraction', () => {
    const onChange = jest.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={onChange}
      />
    )
    fireEvent.change(screen.getByTestId('slider'), { target: { value: '40' } })
    expect(onChange).toHaveBeenCalledWith(0.4)
  })

  test('select node uses translated label and forwards selected value', () => {
    const onChange = jest.fn()
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Mode',
            path: 'mode',
            options: [{ label: 'Auto', labelKey: 'settings.auto', value: 'auto' }]
          } as any
        }
        value="auto"
        onChange={onChange}
      />
    )
    expect(screen.getByText('t:settings.auto')).toBeInTheDocument()
    fireEvent.change(screen.getByTestId('select'), { target: { value: 'auto' } })
    expect(onChange).toHaveBeenCalledWith('auto')
  })

  test('color node uses default color and supports reset', () => {
    const onChange = jest.fn()
    const { rerender } = render(
      <SettingsFieldControl
        node={{ type: 'color', label: 'Highlight', path: 'highlightColorDark' } as any}
        value={null}
        onChange={onChange}
      />
    )
    expect(screen.getByTestId('textfield-color')).toHaveValue('#ffffff')
    expect(screen.getByTestId('icon-button')).toBeDisabled()

    rerender(
      <SettingsFieldControl
        node={{ type: 'color', label: 'Highlight', path: 'highlightColorDark' } as any}
        value="#ffffff"
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('icon-button'))
    expect(onChange).toHaveBeenCalledWith(null)
  })
})
