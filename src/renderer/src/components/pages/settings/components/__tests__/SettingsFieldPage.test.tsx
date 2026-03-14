import { render, screen } from '@testing-library/react'
import { SettingsFieldPage } from '../SettingsFieldPage'

jest.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => `t:${k}` })
}))

jest.mock('../SettingsFieldControl', () => ({
  SettingsFieldControl: () => <div data-testid="field-control" />
}))

describe('SettingsFieldPage', () => {
  test('renders translated page description', () => {
    render(
      <SettingsFieldPage
        node={
          {
            type: 'string',
            path: 'name',
            label: 'Name',
            page: { labelDescription: 'settings.name.desc' }
          } as any
        }
        value="x"
        onChange={jest.fn()}
      />
    )

    expect(screen.getByTestId('field-control')).toBeInTheDocument()
    expect(screen.getByText('t:settings.name.desc')).toBeInTheDocument()
  })

  test('does not render description when not provided', () => {
    render(
      <SettingsFieldPage
        node={{ type: 'string', path: 'name', label: 'Name' } as any}
        value="x"
        onChange={jest.fn()}
      />
    )
    expect(screen.getByTestId('field-control')).toBeInTheDocument()
    expect(screen.queryByText('t:settings.name.desc')).toBeNull()
  })
})
