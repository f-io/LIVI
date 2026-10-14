import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'

vi.mock('../../../widgets', () => ({
  NavFull: () => <div>NavFull</div>
}))

vi.mock('../../dashboard1/DashboardFrame', () => ({
  DashboardFrame: ({ clusterFull, children }: { clusterFull?: boolean; children?: ReactNode }) => (
    <div>
      <span>clusterFull:{clusterFull ? 'yes' : 'no'}</span>
      <span>{children}</span>
    </div>
  )
}))

import { Dashboard2 } from '../Dashboard2'

describe('Dashboard2', () => {
  test('renders the full nav inside a plain DashboardFrame (no cluster cut-out)', () => {
    render(<Dashboard2 />)
    expect(screen.getByText('clusterFull:no')).toBeInTheDocument()
    expect(screen.getByText('NavFull')).toBeInTheDocument()
  })
})
