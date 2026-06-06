import { render, screen } from '@testing-library/react'
import type { ReactNode } from 'react'

jest.mock('../../dash1/NavMiniCenter', () => ({
  NavMiniCenter: () => <div>MiniNav</div>
}))

jest.mock('../../dash1/DashFrame', () => ({
  DashFrame: ({ backdropMask, children }: { backdropMask?: string; children?: ReactNode }) => (
    <div>
      <span>mask:{backdropMask ? 'yes' : 'no'}</span>
      <span>{children}</span>
    </div>
  )
}))

import { Dash2 } from '../Dash2'

describe('Dash2', () => {
  test('renders the mini-nav, no cluster cut-out', () => {
    render(<Dash2 />)
    expect(screen.getByText('mask:no')).toBeInTheDocument()
    expect(screen.getByText('MiniNav')).toBeInTheDocument()
  })
})
