import type { SettingsNode } from '@renderer/routes/types'
import type { ExtraConfig } from '@shared/types'
import { fireEvent, render, screen } from '@testing-library/react'
import { StackItem } from '../StackItem'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, fallback?: string) => `t:${key}:${fallback ?? ''}`
  })
}))

describe('StackItem', () => {
  test('renders translated label for select option', () => {
    const node = {
      type: 'select',
      label: 'Theme',
      path: 'theme',
      options: [{ value: 'light', label: 'Light', labelKey: 'settings.theme.light' }]
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value="light">
        <span>Theme</span>
      </StackItem>
    )

    expect(screen.getByText('t:settings.theme.light:Light')).toBeInTheDocument()
  })

  test('shows fallback --- for null-like formatted values', () => {
    const node = {
      type: 'number',
      label: 'Speed',
      path: 'speed',
      valueTransform: { format: () => 'undefined' }
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value={42}>
        <span>Speed</span>
      </StackItem>
    )

    expect(screen.getByText('---')).toBeInTheDocument()
  })

  test('invokes onClick on Enter and Space keys', () => {
    const onClick = jest.fn()
    render(
      <StackItem onClick={onClick}>
        <span>Open</span>
      </StackItem>
    )

    const el = screen.getByRole('button')
    fireEvent.keyDown(el, { key: 'Enter' })
    fireEvent.keyDown(el, { key: ' ' })

    expect(onClick).toHaveBeenCalledTimes(2)
  })

  test('shows empty value when select option is not found', () => {
    const node = {
      type: 'select',
      label: 'Theme',
      path: 'theme',
      options: [{ value: 'light', label: 'Light', labelKey: 'settings.theme.light' }]
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value="dark">
        <span>Theme</span>
      </StackItem>
    )

    const valueContainer = screen.getByText('Theme').parentElement
    expect(valueContainer).toBeInTheDocument()
    expect(screen.queryByText('t:settings.theme.light:Light')).not.toBeInTheDocument()
  })

  test('shows fallback --- for null-like string "null"', () => {
    const node = {
      type: 'number',
      label: 'Speed',
      path: 'speed',
      valueTransform: { format: () => 'null' }
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value={42}>
        <span>Speed</span>
      </StackItem>
    )

    expect(screen.getByText('---')).toBeInTheDocument()
  })

  test('does not invoke onClick for other keys', () => {
    const onClick = jest.fn()

    render(
      <StackItem onClick={onClick}>
        <span>Open</span>
      </StackItem>
    )

    const el = screen.getByRole('button')
    fireEvent.keyDown(el, { key: 'Escape' })

    expect(onClick).not.toHaveBeenCalled()
  })

  test('does not get button role or tab focus without onClick', () => {
    render(
      <StackItem>
        <span>Static item</span>
      </StackItem>
    )

    expect(screen.queryByRole('button')).not.toBeInTheDocument()
  })

  test('uses valueTransform.toView before formatting', () => {
    const node = {
      type: 'number',
      label: 'Speed',
      path: 'speed',
      valueTransform: {
        toView: (v: number) => v / 2,
        format: (v: number) => `${v} km/h`
      }
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value={100}>
        <span>Speed</span>
      </StackItem>
    )

    expect(screen.getByText('50 km/h')).toBeInTheDocument()
  })

  test('renders plain option label when select option has no labelKey', () => {
    const node = {
      type: 'select',
      label: 'Theme',
      path: 'theme',
      options: [{ value: 'light', label: 'Light' }]
    } as SettingsNode<ExtraConfig>

    render(
      <StackItem node={node} showValue value="light">
        <span>Theme</span>
      </StackItem>
    )

    expect(screen.getByText('Light')).toBeInTheDocument()
  })

  test('ignores Enter key when onClick is not provided', () => {
    render(
      <StackItem>
        <span>Static item</span>
      </StackItem>
    )

    fireEvent.keyDown(screen.getByText('Static item'), { key: 'Enter' })
    expect(screen.getByText('Static item')).toBeInTheDocument()
  })

  test('renders forward icon when requested', () => {
    const { container } = render(
      <StackItem withForwardIcon>
        <span>Forward item</span>
      </StackItem>
    )

    expect(container.querySelector('svg')).toBeInTheDocument()
  })
})
