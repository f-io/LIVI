import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { PosSensitiveList } from '../PosSensitiveList'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => fallback ?? key
  })
}))

jest.mock('@mui/material', () => ({
  Typography: ({ children }: { children: React.ReactNode }) =>
    React.createElement('span', null, children),
  IconButton: ({
    children,
    disabled,
    onClick
  }: {
    children: React.ReactNode
    disabled?: boolean
    onClick?: () => void
  }) =>
    React.createElement(
      'button',
      {
        type: 'button',
        disabled,
        onClick
      },
      children
    ),
  Switch: ({
    checked,
    onChange
  }: {
    checked: boolean
    onChange?: (_event: unknown, checked: boolean) => void
  }) =>
    React.createElement('input', {
      type: 'checkbox',
      checked,
      onChange: (e: React.ChangeEvent<HTMLInputElement>) => onChange?.(e, e.target.checked)
    })
}))

jest.mock('@mui/icons-material/ExpandLess', () => ({
  __esModule: true,
  default: () => React.createElement('span', null, 'up')
}))

jest.mock('@mui/icons-material/ExpandMore', () => ({
  __esModule: true,
  default: () => React.createElement('span', null, 'down')
}))

jest.mock('../../stackItem', () => ({
  StackItem: ({ children }: { children: React.ReactNode }) =>
    React.createElement('div', null, children)
}))

describe('PosSensitiveList', () => {
  const node = {
    items: [
      { id: 'dash1', label: 'Dash 1', labelKey: 'dash.1' },
      { id: 'dash2', label: 'Dash 2' },
      { id: 'dash3', label: 'Dash 3' },
      { id: 'dash4', label: 'Dash 4' },
      { id: 'not-a-dash', label: 'Other' }
    ]
  }

  test('renders defaults when value is not an array', () => {
    const onChange = jest.fn()

    render(<PosSensitiveList node={node as never} value={undefined as never} onChange={onChange} />)

    expect(screen.getByText('Dash 1')).toBeInTheDocument()
    expect(screen.getByText('Dash 2')).toBeInTheDocument()
    expect(screen.getByText('Dash 3')).toBeInTheDocument()
    expect(screen.getByText('Dash 4')).toBeInTheDocument()
    expect(screen.queryByText('Other')).not.toBeInTheDocument()

    const switches = screen.getAllByRole('checkbox')
    expect(switches).toHaveLength(4)
    switches.forEach((sw) => expect(sw).not.toBeChecked())
  })

  test('uses translation fallback label resolution', () => {
    const onChange = jest.fn()

    render(<PosSensitiveList node={node as never} value={undefined as never} onChange={onChange} />)

    expect(screen.getByText('Dash 1')).toBeInTheDocument()
    expect(screen.getByText('Dash 2')).toBeInTheDocument()
  })

  test('normalizes stored dashboards, ignores invalid entries and renumbers positions', () => {
    const onChange = jest.fn()

    render(
      <PosSensitiveList
        node={node as never}
        value={
          [
            null,
            { id: 'dash3', enabled: true, pos: 99 },
            { id: 'dash1', enabled: false, pos: 5.8 },
            { id: 'dash2', enabled: true, pos: 5.2 },
            { id: 'bad-id', enabled: true, pos: 1 },
            { id: 'dash4', enabled: false, pos: Number.NaN }
          ] as never
        }
        onChange={onChange}
      />
    )

    const labels = screen.getAllByText(/Dash /).map((x) => x.textContent)
    expect(labels).toEqual(['Dash 4', 'Dash 2', 'Dash 1', 'Dash 3'])

    const switches = screen.getAllByRole('checkbox')
    expect(switches[0]).not.toBeChecked()
    expect(switches[1]).toBeChecked()
    expect(switches[2]).not.toBeChecked()
    expect(switches[3]).toBeChecked()
  })

  test('toggle switch updates enabled state and emits sorted payload', () => {
    const onChange = jest.fn()

    render(
      <PosSensitiveList
        node={node as never}
        value={
          [
            { id: 'dash1', enabled: false, pos: 1 },
            { id: 'dash2', enabled: false, pos: 2 },
            { id: 'dash3', enabled: false, pos: 3 },
            { id: 'dash4', enabled: false, pos: 4 }
          ] as never
        }
        onChange={onChange}
      />
    )

    fireEvent.click(screen.getAllByRole('checkbox')[1])

    expect(onChange).toHaveBeenCalledWith([
      { id: 'dash1', enabled: false, pos: 1 },
      { id: 'dash2', enabled: true, pos: 2 },
      { id: 'dash3', enabled: false, pos: 3 },
      { id: 'dash4', enabled: false, pos: 4 }
    ])
  })

  test('move down swaps positions and emits sorted payload', () => {
    const onChange = jest.fn()

    render(
      <PosSensitiveList
        node={node as never}
        value={
          [
            { id: 'dash1', enabled: false, pos: 1 },
            { id: 'dash2', enabled: false, pos: 2 },
            { id: 'dash3', enabled: false, pos: 3 },
            { id: 'dash4', enabled: false, pos: 4 }
          ] as never
        }
        onChange={onChange}
      />
    )

    const buttons = screen.getAllByRole('button')
    fireEvent.click(buttons[1])

    expect(onChange).toHaveBeenCalledWith([
      { id: 'dash2', enabled: false, pos: 1 },
      { id: 'dash1', enabled: false, pos: 2 },
      { id: 'dash3', enabled: false, pos: 3 },
      { id: 'dash4', enabled: false, pos: 4 }
    ])
  })

  test('move up swaps positions and emits sorted payload', () => {
    const onChange = jest.fn()

    render(
      <PosSensitiveList
        node={node as never}
        value={
          [
            { id: 'dash1', enabled: false, pos: 1 },
            { id: 'dash2', enabled: false, pos: 2 },
            { id: 'dash3', enabled: false, pos: 3 },
            { id: 'dash4', enabled: false, pos: 4 }
          ] as never
        }
        onChange={onChange}
      />
    )

    const buttons = screen.getAllByRole('button')
    fireEvent.click(buttons[2])

    expect(onChange).toHaveBeenCalledWith([
      { id: 'dash2', enabled: false, pos: 1 },
      { id: 'dash1', enabled: false, pos: 2 },
      { id: 'dash3', enabled: false, pos: 3 },
      { id: 'dash4', enabled: false, pos: 4 }
    ])
  })

  test('disables move up for first item and move down for last item', () => {
    const onChange = jest.fn()

    render(<PosSensitiveList node={node as never} value={undefined as never} onChange={onChange} />)

    const buttons = screen.getAllByRole('button')
    expect(buttons[0]).toBeDisabled()
    expect(buttons[buttons.length - 1]).toBeDisabled()
  })
})
