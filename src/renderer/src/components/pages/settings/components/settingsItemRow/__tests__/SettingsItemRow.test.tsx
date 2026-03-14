import { render, screen } from '@testing-library/react'
import { SettingsItemRow } from '../SettingsItemRow'

jest.mock('../../stackItem', () => ({
  StackItem: ({ children }: any) => <div data-testid="stack-item">{children}</div>
}))

describe('SettingsItemRow', () => {
  test('renders label and children inside StackItem', () => {
    render(
      <SettingsItemRow label="Audio">
        <span>Control</span>
      </SettingsItemRow>
    )
    expect(screen.getByTestId('stack-item')).toBeInTheDocument()
    expect(screen.getByText('Audio')).toBeInTheDocument()
    expect(screen.getByText('Control')).toBeInTheDocument()
  })
})
