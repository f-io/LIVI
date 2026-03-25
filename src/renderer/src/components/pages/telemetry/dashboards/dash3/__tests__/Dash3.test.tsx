import { render, screen } from '@testing-library/react'
import { Dash3 } from '../Dash3'

jest.mock('../../../components/DashPlaceholder', () => ({
  DashPlaceholder: ({ title }: { title: string }) => <div>{title}</div>
}))

describe('Dash3', () => {
  test('renders DashPlaceholder with Dash 3 title', () => {
    render(<Dash3 />)

    expect(screen.getByText('Dash 3')).toBeInTheDocument()
  })
})
