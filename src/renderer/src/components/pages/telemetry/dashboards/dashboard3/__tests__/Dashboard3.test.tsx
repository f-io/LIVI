import { render, screen } from '@testing-library/react'
import { Dashboard3 } from '../Dashboard3'

vi.mock('../../dashboard1/DashboardFrame', () => ({
  DashboardFrame: ({ clusterFull }: { clusterFull?: boolean }) => (
    <div>DashboardFrame:{String(clusterFull)}</div>
  )
}))

describe('Dashboard3', () => {
  test('renders a full-cluster DashboardFrame', () => {
    render(<Dashboard3 />)

    expect(screen.getByText('DashboardFrame:true')).toBeInTheDocument()
  })
})
