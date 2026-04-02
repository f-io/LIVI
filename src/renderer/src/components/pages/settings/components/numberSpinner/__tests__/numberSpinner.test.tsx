import { fireEvent, render, screen } from '@testing-library/react'
import React from 'react'
import { AppContext } from '../../../../../../context'
import NumberSpinner from '../numberSpinner'

const alphaMock = jest.fn((color: string, value: number) => `alpha(${color},${value})`)

jest.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    shape: { borderRadius: 8 },
    palette: {
      primary: { main: '#1976d2' },
      divider: '#cccccc',
      text: { primary: '#111111', secondary: '#666666' },
      background: { paper: '#ffffff' }
    }
  }),
  alpha: (color: string, value: number) => alphaMock(color, value)
}))

jest.mock('@mui/material/Box', () => ({
  __esModule: true,
  default: React.forwardRef(function MockBox(
    props: React.HTMLAttributes<HTMLElement> & { component?: React.ElementType },
    ref: React.ForwardedRef<HTMLElement>
  ) {
    const { children, component, ...rest } = props
    const Tag = (component ?? 'div') as React.ElementType
    return React.createElement(Tag, { ...rest, ref }, children)
  })
}))

jest.mock('@mui/material/Button', () => ({
  __esModule: true,
  default: ({
    children,
    ...props
  }: React.ButtonHTMLAttributes<HTMLButtonElement> & { children: React.ReactNode }) =>
    React.createElement('button', { type: 'button', ...props }, children)
}))

jest.mock('@mui/material/FormControl', () => ({
  __esModule: true,
  default: React.forwardRef(function MockFormControl(
    props: React.HTMLAttributes<HTMLDivElement>,
    ref: React.ForwardedRef<HTMLDivElement>
  ) {
    const { children, ...rest } = props
    return React.createElement('div', { ...rest, ref }, children)
  })
}))

jest.mock('@mui/material/FormLabel', () => ({
  __esModule: true,
  default: ({
    children,
    htmlFor,
    ...props
  }: React.LabelHTMLAttributes<HTMLLabelElement> & { children: React.ReactNode }) =>
    React.createElement('label', { htmlFor, ...props }, children)
}))

jest.mock('@mui/material/OutlinedInput', () => ({
  __esModule: true,
  default: ({
    inputRef,
    value,
    onBlur,
    onChange,
    onKeyUp,
    onKeyDown,
    onFocus,
    inputProps,
    slotProps
  }: {
    inputRef?: React.Ref<HTMLInputElement>
    value?: string
    onBlur?: React.FocusEventHandler<HTMLInputElement>
    onChange?: React.ChangeEventHandler<HTMLInputElement>
    onKeyUp?: React.KeyboardEventHandler<HTMLInputElement>
    onKeyDown?: React.KeyboardEventHandler<HTMLInputElement>
    onFocus?: React.FocusEventHandler<HTMLInputElement>
    inputProps?: { readOnly?: boolean }
    slotProps?: {
      input?: React.InputHTMLAttributes<HTMLInputElement> & {
        id?: string
        'aria-label'?: string
        readOnly?: boolean
      }
    }
  }) =>
    React.createElement('input', {
      ref: inputRef,
      value,
      onBlur,
      onChange,
      onKeyUp,
      onKeyDown,
      onFocus,
      readOnly: inputProps?.readOnly ?? slotProps?.input?.readOnly,
      id: slotProps?.input?.id,
      'aria-label': slotProps?.input?.['aria-label']
    })
}))

jest.mock('@mui/icons-material/Add', () => ({
  __esModule: true,
  default: () => React.createElement('span', null, '+')
}))

jest.mock('@mui/icons-material/Remove', () => ({
  __esModule: true,
  default: () => React.createElement('span', null, '-')
}))

jest.mock('@mui/icons-material/OpenInFull', () => ({
  __esModule: true,
  default: () => React.createElement('span', null, 'scrub')
}))

jest.mock('@base-ui/react/number-field', () => {
  const React = require('react')

  const Root = ({
    children,
    render,
    disabled,
    required
  }: {
    children: React.ReactNode
    render: (props: { ref: null; children: React.ReactNode }, state: unknown) => React.ReactNode
    disabled?: boolean
    required?: boolean
  }) => render({ ref: null, children }, { disabled: !!disabled, required: !!required })

  ;(Root as any).Props = {}

  const Input = ({
    render
  }: {
    render: (
      props: {
        ref: null
        onBlur: jest.Mock
        onChange: React.ChangeEventHandler<HTMLInputElement>
        onKeyUp: jest.Mock
        onKeyDown: jest.Mock
        onFocus: jest.Mock
      },
      state: { inputValue: string }
    ) => React.ReactNode
  }) => {
    const [value, setValue] = React.useState('42')

    return render(
      {
        ref: null,
        onBlur: jest.fn(),
        onChange: (e: React.ChangeEvent<HTMLInputElement>) => setValue(e.target.value),
        onKeyUp: jest.fn(),
        onKeyDown: jest.fn(),
        onFocus: jest.fn()
      },
      { inputValue: value }
    )
  }

  const Increment = ({
    children,
    render
  }: {
    children: React.ReactNode
    render: React.ReactElement
  }) => React.cloneElement(render, {}, children)

  const Decrement = ({
    children,
    render
  }: {
    children: React.ReactNode
    render: React.ReactElement
  }) => React.cloneElement(render, {}, children)

  const ScrubArea = ({
    children,
    render
  }: {
    children: React.ReactNode
    render: React.ReactElement
  }) => React.cloneElement(render, {}, children)

  const ScrubAreaCursor = ({ children }: { children: React.ReactNode }) =>
    React.createElement('span', null, children)

  return {
    NumberField: {
      Root,
      Input,
      Increment,
      Decrement,
      ScrubArea,
      ScrubAreaCursor
    }
  }
})

describe('NumberSpinner', () => {
  const renderWithContext = (ui: React.ReactElement, focusedElId: string | null = null) =>
    render(
      <AppContext.Provider
        value={
          {
            keyboardNavigation: {
              focusedElId
            }
          } as never
        }
      >
        {ui}
      </AppContext.Provider>
    )

  beforeEach(() => {
    alphaMock.mockClear()
  })

  test('renders label, input and increment/decrement buttons', () => {
    renderWithContext(
      <NumberSpinner label="Volume" min={0} max={100} value={42} onValueChange={() => {}} />
    )

    expect(screen.getByText('Volume')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Increase' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Decrease' })).toBeInTheDocument()
    expect(screen.getByRole('textbox')).toBeInTheDocument()
  })

  test('uses provided id for label and input aria-label', () => {
    renderWithContext(
      <NumberSpinner
        id="volume-spinner"
        label="Volume"
        min={0}
        max={100}
        value={42}
        onValueChange={() => {}}
      />
    )

    const input = screen.getByLabelText('volume-spinner')
    expect(input).toHaveAttribute('id', 'volume-spinner')
  })

  test('renders scrub area when enableScrub is true', () => {
    renderWithContext(
      <NumberSpinner
        label="Scrubbable"
        enableScrub
        min={0}
        max={100}
        value={42}
        onValueChange={() => {}}
      />
    )

    expect(screen.getByText('Scrubbable')).toBeInTheDocument()
    expect(screen.getByText('scrub')).toBeInTheDocument()
  })

  test('marks input readonly when used as slider', () => {
    renderWithContext(
      <NumberSpinner
        id="slider-spinner"
        label="Slider"
        isSlider
        min={0}
        max={100}
        value={25}
        onValueChange={() => {}}
      />
    )

    expect(screen.getByLabelText('slider-spinner')).toHaveAttribute('readonly')
  })

  test('does not mark input readonly when not used as slider', () => {
    renderWithContext(
      <NumberSpinner
        id="normal-spinner"
        label="Normal"
        min={0}
        max={100}
        value={25}
        onValueChange={() => {}}
      />
    )

    expect(screen.getByLabelText('normal-spinner')).not.toHaveAttribute('readonly')
  })

  test('shows armed color path when focused element id matches component id', () => {
    renderWithContext(
      <NumberSpinner
        id="armed-spinner"
        label="Armed"
        min={0}
        max={100}
        value={25}
        onValueChange={() => {}}
      />,
      'armed-spinner'
    )

    expect(screen.getByLabelText('armed-spinner')).toBeInTheDocument()
  })

  test('calls alpha helper when slider overlay is rendered', () => {
    renderWithContext(
      <NumberSpinner
        id="alpha-spinner"
        label="Alpha"
        isSlider
        min={0}
        max={100}
        value={35}
        onValueChange={() => {}}
      />
    )

    expect(alphaMock).toHaveBeenCalledWith('#1976d2', 0.25)
  })

  test('input value can change through mocked number field input change', () => {
    renderWithContext(
      <NumberSpinner
        id="editable-spinner"
        label="Editable"
        min={0}
        max={100}
        value={42}
        onValueChange={() => {}}
      />
    )

    const input = screen.getByLabelText('editable-spinner') as HTMLInputElement
    fireEvent.change(input, { target: { value: '77' } })

    expect(input.value).toBe('77')
  })
})
