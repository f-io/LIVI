import { render, screen } from '@testing-library/react'

jest.mock('../components/secondaryWindow/SecondaryAppShell', () => ({
  SecondaryAppShell: ({ role, emptyLabel }: { role: string; emptyLabel: string }) => (
    <div data-testid="shell" data-role={role}>
      {emptyLabel}
    </div>
  )
}))

import AuxApp from '../AuxApp'

describe('AuxApp', () => {
  test('mounts SecondaryAppShell with role=aux and the Aux empty label', () => {
    render(<AuxApp />)
    const shell = screen.getByTestId('shell')
    expect(shell).toHaveAttribute('data-role', 'aux')
    expect(shell).toHaveTextContent('Aux Window')
  })
})
