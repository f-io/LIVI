import { render, screen } from '@testing-library/react'

jest.mock('../components/secondaryWindow/SecondaryAppShell', () => ({
  SecondaryAppShell: ({ role, emptyLabel }: { role: string; emptyLabel: string }) => (
    <div data-testid="shell" data-role={role}>
      {emptyLabel}
    </div>
  )
}))

import DashApp from '../DashApp'

describe('DashApp', () => {
  test('mounts SecondaryAppShell with role=dash and the Dash empty label', () => {
    render(<DashApp />)
    const shell = screen.getByTestId('shell')
    expect(shell).toHaveAttribute('data-role', 'dash')
    expect(shell).toHaveTextContent('Dash Window')
  })
})
