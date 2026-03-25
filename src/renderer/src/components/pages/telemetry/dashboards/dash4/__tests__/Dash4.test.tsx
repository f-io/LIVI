import { render, screen } from '@testing-library/react'
import { Dash4 } from '../Dash4'

jest.mock('../../../widgets', () => ({
  NavFull: () => <div>NavFull</div>
}))

describe('Dash4', () => {
  test('renders NavFull component', () => {
    render(<Dash4 />)

    expect(screen.getByText('NavFull')).toBeInTheDocument()
  })
})
