import { render, screen } from '@testing-library/react'
import { FuelLevel } from '../FuelLevel'

describe('FuelLevel', () => {
  test('renders rounded fuel level', () => {
    render(<FuelLevel fuelPct={54.6} />)

    expect(screen.getByText('FUEL')).toBeInTheDocument()
    expect(screen.getByText('55')).toBeInTheDocument()
    expect(screen.getByText('%')).toBeInTheDocument()
  })

  test('clamps values below 0', () => {
    render(<FuelLevel fuelPct={-5} />)

    expect(screen.getByText('0')).toBeInTheDocument()
  })

  test('clamps values above 100', () => {
    render(<FuelLevel fuelPct={150} />)

    expect(screen.getByText('100')).toBeInTheDocument()
  })

  test('falls back to 0 for non-finite values', () => {
    render(<FuelLevel fuelPct={Number.NaN} />)

    expect(screen.getByText('0')).toBeInTheDocument()
  })

  test('applies className to root element', () => {
    const { container } = render(<FuelLevel fuelPct={50} className="fuel-test" />)

    expect(container.firstChild).toHaveClass('fuel-test')
  })

  test('renders default value when no fuel level is provided', () => {
    render(<FuelLevel />)

    expect(screen.getByText('FUEL')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
    expect(screen.getByText('%')).toBeInTheDocument()
  })
})
