import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'

jest.mock('../../../widgets', () => ({
  NavFull: () => <div>NavFull</div>
}))

jest.mock('../../dash1/DashFrame', () => ({
  DashFrame: ({ clusterFull, children }: { clusterFull?: boolean; children?: ReactNode }) => (
    <div>
      <span>clusterFull:{clusterFull ? 'yes' : 'no'}</span>
      <span>{children}</span>
    </div>
  )
}))

import { Dash2 } from '../Dash2'

describe('Dash2', () => {
  test('renders the full nav inside a plain DashFrame (no cluster cut-out)', () => {
    render(<Dash2 />)
    expect(screen.getByText('clusterFull:no')).toBeInTheDocument()
    expect(screen.getByText('NavFull')).toBeInTheDocument()
  })
})
