import { render, screen } from '@testing-library/react'
import { CoolantTemp } from '../CoolantTemp'

describe('CoolantTemp', () => {
  test('renders rounded coolant temperature', () => {
    render(<CoolantTemp coolantC={92.6} />)

    expect(screen.getByText('COOLANT')).toBeInTheDocument()
    expect(screen.getByText('93')).toBeInTheDocument()
    expect(screen.getByText('°C')).toBeInTheDocument()
  })

  test('clamps values below -99', () => {
    render(<CoolantTemp coolantC={-120} />)

    expect(screen.getByText('-99')).toBeInTheDocument()
  })

  test('clamps values above 999', () => {
    render(<CoolantTemp coolantC={1200} />)

    expect(screen.getByText('999')).toBeInTheDocument()
  })

  test('falls back to 0 for non-finite values', () => {
    render(<CoolantTemp coolantC={Number.NaN} />)

    expect(screen.getByText('0')).toBeInTheDocument()
  })

  test('applies className to root element', () => {
    const { container } = render(<CoolantTemp coolantC={90} className="coolant-test" />)

    expect(container.firstChild).toHaveClass('coolant-test')
  })

  test('renders default value when no coolant temperature is provided', () => {
    render(<CoolantTemp />)

    expect(screen.getByText('COOLANT')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
    expect(screen.getByText('°C')).toBeInTheDocument()
  })
})
