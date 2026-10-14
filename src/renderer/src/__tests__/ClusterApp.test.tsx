import { render, screen } from '@testing-library/react'

vi.mock('../components/secondaryWindow/SecondaryAppShell', () => ({
  SecondaryAppShell: ({ role, emptyLabel }: { role: string; emptyLabel: string }) => (
    <div data-testid="shell" data-role={role}>
      {emptyLabel}
    </div>
  )
}))

import ClusterApp from '../ClusterApp'

describe('ClusterApp', () => {
  test('mounts SecondaryAppShell with role=cluster and the Cluster empty label', () => {
    render(<ClusterApp />)
    const shell = screen.getByTestId('shell')
    expect(shell).toHaveAttribute('data-role', 'cluster')
    expect(shell).toHaveTextContent('Cluster Window')
  })
})
