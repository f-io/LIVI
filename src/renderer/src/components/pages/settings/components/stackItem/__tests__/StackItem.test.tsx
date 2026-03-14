import { fireEvent, render, screen } from '@testing-library/react'
import { StackItem } from '../StackItem'
import type { SettingsNode } from '@renderer/routes/types'
import type { ExtraConfig } from '@shared/types'

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
})
