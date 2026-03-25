import { render, screen } from '@testing-library/react'
import { Dash2 } from '../Dash2'

jest.mock('../../../components/DashPlaceholder', () => ({
  DashPlaceholder: ({ title }: { title: string }) => <div>{title}</div>
}))

describe('Dash2', () => {
  test('renders DashPlaceholder with Dash 2 title', () => {
    render(<Dash2 />)

    expect(screen.getByText('Dash 2')).toBeInTheDocument()
  })
})
