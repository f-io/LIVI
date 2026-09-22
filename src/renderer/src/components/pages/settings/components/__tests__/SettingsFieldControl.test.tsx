import { themeColors } from '@renderer/theme/themeColors'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { SettingsFieldControl } from '../SettingsFieldControl'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => `t:${k}` })
}))

let capturedSlider: any = null

vi.mock('@mui/material', async () => {
  const actual = await vi.importActual('@mui/material')
  return {
    ...actual,
    TextField: ({ value, onChange, onBlur, onFocus, onKeyDown, type = 'text' }: any) => (
      <input
        data-testid={`textfield-${type}`}
        type={type}
        value={value}
        onChange={onChange}
        onBlur={onBlur}
        onFocus={onFocus}
        onKeyDown={onKeyDown}
      />
    ),
    Switch: ({ checked, onChange }: any) => (
      <input
        data-testid="switch"
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e, e.currentTarget.checked)}
      />
    ),
    Slider: (props: any) => {
      capturedSlider = props
      return (
        <input
          data-testid="slider"
          type="range"
          value={props.value}
          onChange={(e) => {
            const next = Number(e.currentTarget.value)
            props.onChange?.(e, next)
            props.onChangeCommitted?.(e, next)
          }}
        />
      )
    },
    IconButton: ({ onClick, disabled, children }: any) => (
      <button data-testid="icon-button" disabled={disabled} onClick={onClick}>
        {children}
      </button>
    )
  }
})

const mockGetCached = vi.fn()
const mockResolve = vi.fn()

vi.mock('../selectOptionsCache', () => ({
  getCachedOptions: (...args: unknown[]) => mockGetCached(...args),
  resolveOptions: (...args: unknown[]) => mockResolve(...args)
}))

vi.mock('../numberSpinner/numberSpinner', () => ({
  __esModule: true,
  default: ({ onValueCommitted }: { onValueCommitted: (n: number) => void }) => (
    <div>
      <button data-testid="spinner-ok" onClick={() => onValueCommitted(42.9)} />
      <button data-testid="spinner-bad" onClick={() => onValueCommitted(Number.NaN)} />
    </div>
  )
}))

describe('SettingsFieldControl', () => {
  beforeEach(() => {
    mockGetCached.mockReset()
    mockGetCached.mockReturnValue(undefined)
    mockResolve.mockReset()
    mockResolve.mockResolvedValue([])
    capturedSlider = null
    ;(window as any).projection = undefined
  })

  test('string node commits on blur, not per keystroke', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="old"
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'new' } })
    expect(onChange).not.toHaveBeenCalled() //no write while typing
    fireEvent.blur(input)
    expect(onChange).toHaveBeenCalledWith('new')
  })

  test('string node keeps the draft when the stored value changes while editing', () => {
    const onChange = vi.fn()
    const { rerender } = render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="old"
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'typing' } })

    rerender(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="from elsewhere"
        onChange={onChange}
      />
    )

    expect((input as HTMLInputElement).value).toBe('typing')
  })

  test('a url node refuses a half-typed address and takes a bare host', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Address', path: 'customUrl', format: 'url' } as any}
        value=""
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')

    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'h' } })
    fireEvent.blur(input)
    expect(onChange).not.toHaveBeenCalled()

    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: '10.0.0.9' } })
    fireEvent.blur(input)
    expect(onChange).toHaveBeenCalledWith('10.0.0.9')
  })

  test('string node stays put on any other key', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="old"
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'new' } })
    fireEvent.keyDown(input, { key: 'a' })

    expect(onChange).not.toHaveBeenCalled()
  })

  test('string node commits on Enter', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value="old"
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'new' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    fireEvent.blur(input)
    expect(onChange).toHaveBeenCalledWith('new')
  })

  test('url field blocks the commit on an invalid URL and allows a valid one', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Address', path: 'customUrl', format: 'url' } as any}
        value=""
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'notaurl' } })
    fireEvent.blur(input)
    expect(onChange).not.toHaveBeenCalled() //invalid -> no write
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: 'http://livi.local/dash' } })
    fireEvent.blur(input)
    expect(onChange).toHaveBeenCalledWith('http://livi.local/dash')
  })

  test('url field accepts empty to clear', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Address', path: 'customUrl', format: 'url' } as any}
        value="http://old"
        onChange={onChange}
      />
    )
    const input = screen.getByTestId('textfield-text')
    fireEvent.focus(input)
    fireEvent.change(input, { target: { value: '' } })
    fireEvent.blur(input)
    expect(onChange).toHaveBeenCalledWith('')
  })

  test('string node flags too-short values against minLength', () => {
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name', minLength: 5 } as any}
        value="ab"
        onChange={vi.fn()}
      />
    )
    expect(screen.getByTestId('textfield-text')).toBeInTheDocument()
  })

  test('string node accepts values that satisfy minLength', () => {
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name', minLength: 2 } as any}
        value="abcd"
        onChange={vi.fn()}
      />
    )
    expect(screen.getByTestId('textfield-text')).toBeInTheDocument()
  })

  test('string node coerces null value to empty text', () => {
    render(
      <SettingsFieldControl
        node={{ type: 'string', label: 'Name', path: 'name' } as any}
        value={null}
        onChange={vi.fn()}
      />
    )
    expect((screen.getByTestId('textfield-text') as HTMLInputElement).value).toBe('')
  })

  test('number node clamps and ignores non-finite values', () => {
    const onChange = vi.fn()
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

  test('number node applies defaults when min/max absent and value non-numeric', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'number', label: 'FPS', path: 'fps' } as any}
        value={undefined}
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('spinner-ok'))
    expect(onChange).toHaveBeenCalledWith(43)
  })

  test('number node snaps to step when step greater than one', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'number', label: 'FPS', path: 'fps', min: 0, max: 100, step: 5 } as any}
        value={0}
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('spinner-ok'))
    expect(onChange).toHaveBeenCalledWith(45)
  })

  test('checkbox node forwards boolean changes and respects disabled', () => {
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'checkbox', label: 'Mute', path: 'mute', disabled: true } as any}
        value={false}
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByTestId('switch'))
    expect(onChange).toHaveBeenCalledWith(true)
  })

  test('slider node converts 0..100 to fraction', () => {
    const onChange = vi.fn()
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

  test('slider node handles zero and undefined initial values', () => {
    const { rerender } = render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0}
        onChange={vi.fn()}
      />
    )
    expect(screen.getByTestId('slider')).toBeInTheDocument()
    expect(capturedSlider.value).toBe(0)
    rerender(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={undefined}
        onChange={vi.fn()}
      />
    )
    expect(screen.getByTestId('slider')).toBeInTheDocument()
    expect(capturedSlider.value).toBe(100)
    rerender(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={Number.NaN as any}
        onChange={vi.fn()}
      />
    )
    expect(capturedSlider.value).toBe(100)
  })

  test('slider value label shows mute icon at zero and percent otherwise', () => {
    render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={vi.fn()}
      />
    )
    expect(capturedSlider.valueLabelFormat(0)).toBeTruthy()
    expect(capturedSlider.valueLabelFormat(50)).toBe('50%')
  })

  test('slider debounces drag changes via settle timer', () => {
    vi.useFakeTimers()
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={onChange}
      />
    )
    act(() => {
      capturedSlider.onChange({}, 30)
      capturedSlider.onChange({}, 31)
    })
    expect(onChange).not.toHaveBeenCalled()
    act(() => {
      vi.advanceTimersByTime(50)
    })
    expect(onChange).toHaveBeenCalledWith(0.31)
    vi.useRealTimers()
  })

  test('slider commit flushes armed timers immediately', () => {
    vi.useFakeTimers()
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={onChange}
      />
    )
    act(() => {
      capturedSlider.onChange({}, 70)
      capturedSlider.onChangeCommitted({}, 70)
    })
    expect(onChange).toHaveBeenCalledWith(0.7)
    vi.useRealTimers()
  })

  test('slider commit without prior drag flushes with no timers', () => {
    vi.useFakeTimers()
    const onChange = vi.fn()
    render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={onChange}
      />
    )
    act(() => {
      capturedSlider.onChangeCommitted({}, 20)
    })
    expect(onChange).toHaveBeenCalledWith(0.2)
    vi.useRealTimers()
  })

  test('slider ignores external value updates while dragging', () => {
    vi.useFakeTimers()
    const { rerender } = render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={vi.fn()}
      />
    )
    act(() => {
      capturedSlider.onChange({}, 30)
    })
    rerender(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.9}
        onChange={vi.fn()}
      />
    )
    expect(capturedSlider.value).toBe(30)
    vi.useRealTimers()
  })

  test('slider clears armed timers on unmount', () => {
    vi.useFakeTimers()
    const onChange = vi.fn()
    const { unmount } = render(
      <SettingsFieldControl
        node={{ type: 'slider', label: 'Scale', path: 'scale' } as any}
        value={0.5}
        onChange={onChange}
      />
    )
    act(() => {
      capturedSlider.onChange({}, 44)
    })
    unmount()
    act(() => {
      vi.advanceTimersByTime(300)
    })
    expect(onChange).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('select node uses translated label and forwards selected value', () => {
    const onChange = vi.fn()
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
    fireEvent.click(screen.getByText('t:settings.auto'))
    expect(onChange).toHaveBeenCalledWith('auto')
  })

  test('select node seeds options from cache when present', () => {
    mockGetCached.mockReturnValue([{ value: 'cached', label: 'Cached' }])
    render(
      <SettingsFieldControl
        node={{ type: 'select', label: 'Mode', path: 'mode', options: [] } as any}
        value="cached"
        onChange={vi.fn()}
      />
    )
    expect(screen.getByText('Cached')).toBeInTheDocument()
  })

  test('select node marks selected row and leaves others unselected', () => {
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Mode',
            path: 'mode',
            options: [
              { value: 'a', label: 'A' },
              { value: 'b', label: 'B' }
            ]
          } as any
        }
        value="a"
        onChange={vi.fn()}
      />
    )
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
  })

  test('select node with value absent from options selects nothing', () => {
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Mode',
            path: 'mode',
            options: [{ value: 'a', label: 'A' }]
          } as any
        }
        value="missing"
        onChange={vi.fn()}
      />
    )
    expect(screen.getByText('A')).toBeInTheDocument()
  })

  test('select loadOptions migrates saved BT id to the live id', async () => {
    const onChange = vi.fn()
    const live = [{ value: 'bluez_output.AA_BB_CC_DD_EE_FF.2', label: 'Headset', offline: false }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="bluez_output.AA_BB_CC_DD_EE_FF.1"
        onChange={onChange}
      />
    )
    await waitFor(() => expect(onChange).toHaveBeenCalledWith('bluez_output.AA_BB_CC_DD_EE_FF.2'))
  })

  test('select loadOptions does not migrate when no live MAC matches', async () => {
    const onChange = vi.fn()
    const live = [{ value: 'bluez_output.11_22_33_44_55_66.1', label: 'Other', offline: false }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="bluez_output.AA_BB_CC_DD_EE_FF.1"
        onChange={onChange}
      />
    )
    await screen.findByText('Other')
    expect(onChange).not.toHaveBeenCalled()
  })

  test('select loadOptions updates saved label from live option', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'usb1', label: 'USB Speaker', labelKey: 'k.usb' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await waitFor(() => expect(onLabelChange).toHaveBeenCalledWith('t:k.usb'))
  })

  test('select loadOptions relabels using plain label when option has no labelKey', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'usb1', label: 'USB Speaker' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await waitFor(() => expect(onLabelChange).toHaveBeenCalledWith('USB Speaker'))
  })

  test('select loadOptions skips relabel when matched label is empty', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'usb1', label: '' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await waitFor(() => expect(mockResolve).toHaveBeenCalled())
    await act(async () => {})
    expect(onLabelChange).not.toHaveBeenCalled()
  })

  test('select loadOptions skips relabel when value is empty', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'usb1', label: 'USB' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value=""
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await screen.findByText('USB')
    expect(onLabelChange).not.toHaveBeenCalled()
  })

  test('select loadOptions skips relabel when value not among options', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'other', label: 'Other' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await screen.findByText('Other')
    expect(onLabelChange).not.toHaveBeenCalled()
  })

  test('select click on offline non-bluez option does not attempt a connect', () => {
    const onChange = vi.fn()
    const connect = vi.fn()
    ;(window as any).projection = { ipc: { connectBluetoothPairedDevice: connect } }
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [{ value: 'plainOffline', label: 'Plain', offline: true }]
          } as any
        }
        value=""
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByText('t:settings.audioDeviceOffline'))
    expect(onChange).toHaveBeenCalledWith('plainOffline')
    expect(connect).not.toHaveBeenCalled()
  })

  test('select loadOptions keeps existing saved label and skips relabel', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'usb1', label: 'USB Speaker' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        savedLabel="Saved"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await screen.findAllByText('USB Speaker')
    expect(onLabelChange).not.toHaveBeenCalled()
  })

  test('select loadOptions skips relabel when live option missing', async () => {
    const onLabelChange = vi.fn()
    const live = [{ value: 'other', label: 'Other' }]
    mockResolve.mockResolvedValue(live)
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => live
          } as any
        }
        value="usb1"
        savedLabel="Saved"
        onChange={vi.fn()}
        onLabelChange={onLabelChange}
      />
    )
    await screen.findByText('Other')
    expect(onLabelChange).not.toHaveBeenCalled()
  })

  test('select loadOptions bails out when unmounted before resolve', async () => {
    let resolveLater: (opts: unknown[]) => void = () => {}
    mockResolve.mockReturnValue(
      new Promise((r) => {
        resolveLater = r
      })
    )
    const onChange = vi.fn()
    const { unmount } = render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [],
            loadOptions: async () => []
          } as any
        }
        value="usb1"
        onChange={onChange}
      />
    )
    unmount()
    await act(async () => {
      resolveLater([{ value: 'usb1', label: 'USB' }])
    })
    expect(onChange).not.toHaveBeenCalled()
  })

  test('select click connects an offline BT device and calls onDone', async () => {
    const onChange = vi.fn()
    const onDone = vi.fn()
    const connect = vi.fn().mockRejectedValue(new Error('nope'))
    ;(window as any).projection = { ipc: { connectBluetoothPairedDevice: connect } }
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [
              { value: 'bluez_output.AA_BB_CC_DD_EE_FF.1', label: 'Headset', offline: true }
            ]
          } as any
        }
        value=""
        onChange={onChange}
        onDone={onDone}
      />
    )
    fireEvent.click(screen.getByText('t:settings.audioDeviceOffline'))
    expect(onChange).toHaveBeenCalledWith('bluez_output.AA_BB_CC_DD_EE_FF.1')
    expect(onDone).toHaveBeenCalled()
    await waitFor(() => expect(connect).toHaveBeenCalledWith('AA:BB:CC:DD:EE:FF'))
  })

  test('select click on offline BT device without ipc does not throw', () => {
    const onChange = vi.fn()
    ;(window as any).projection = undefined
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [
              { value: 'bluez_output.AA_BB_CC_DD_EE_FF.1', label: 'Headset', offline: true }
            ]
          } as any
        }
        value=""
        onChange={onChange}
      />
    )
    fireEvent.click(screen.getByText('t:settings.audioDeviceOffline'))
    expect(onChange).toHaveBeenCalledWith('bluez_output.AA_BB_CC_DD_EE_FF.1')
  })

  test('select click updates label from a live option', () => {
    const onChange = vi.fn()
    const onLabelChange = vi.fn()
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [{ value: 'a', label: 'Opt A', labelKey: 'k.a' }]
          } as any
        }
        value="a"
        onChange={onChange}
        onLabelChange={onLabelChange}
      />
    )
    fireEvent.click(screen.getByText('t:k.a'))
    expect(onLabelChange).toHaveBeenCalledWith('t:k.a')
  })

  test('select click on a ghost option falls back to the rendered option label', () => {
    const onChange = vi.fn()
    const onLabelChange = vi.fn()
    render(
      <SettingsFieldControl
        node={
          {
            type: 'select',
            label: 'Out',
            path: 'audioOut',
            options: [{ value: 'a', label: 'A' }]
          } as any
        }
        value="ghostval"
        savedLabel="Ghosted"
        onChange={onChange}
        onLabelChange={onLabelChange}
      />
    )
    fireEvent.click(screen.getByText('t:settings.audioDeviceOffline'))
    expect(onChange).toHaveBeenCalledWith('ghostval')
    expect(onLabelChange).toHaveBeenCalledWith('t:settings.audioDeviceOffline')
  })

  test('color node uses default color and supports reset', () => {
    const onChange = vi.fn()
    const { rerender } = render(
      <SettingsFieldControl
        node={{ type: 'color', label: 'Highlight', path: 'highlightColorDark' } as any}
        value={null}
        onChange={onChange}
      />
    )
    expect(
      screen.getByDisplayValue(themeColors.highlightColorDark.toUpperCase())
    ).toBeInTheDocument()
    expect(screen.getByTestId('icon-button')).toBeDisabled()

    rerender(
      <SettingsFieldControl
        node={{ type: 'color', label: 'Highlight', path: 'highlightColorDark' } as any}
        value="#ff0000"
        onChange={onChange}
      />
    )
    expect(screen.getByDisplayValue('#FF0000')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('icon-button'))
    expect(onChange).toHaveBeenCalledWith(null)
  })

  test('unknown node type renders nothing', () => {
    const { container } = render(
      <SettingsFieldControl
        node={{ type: 'mystery', label: 'X', path: 'x' } as any}
        value="x"
        onChange={vi.fn()}
      />
    )
    expect(container.firstChild).toBeNull()
  })
})
