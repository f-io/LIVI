import { render, screen } from '@testing-library/react'
import { OilTemp } from '../OilTemp'

describe('OilTemp', () => {
  test('renders default value when no oil temperature is provided', () => {
    render(<OilTemp />)

    expect(screen.getByText('OIL')).toBeInTheDocument()
    expect(screen.getByText('0')).toBeInTheDocument()
    expect(screen.getByText('°C')).toBeInTheDocument()
  })

  test('renders rounded oil temperature', () => {
    render(<OilTemp oilC={104.6} />)

    expect(screen.getByText('105')).toBeInTheDocument()
  })

  test('clamps values below -99', () => {
    render(<OilTemp oilC={-120} />)

    expect(screen.getByText('-99')).toBeInTheDocument()
  })

  test('clamps values above 999', () => {
    render(<OilTemp oilC={1200} />)

    expect(screen.getByText('999')).toBeInTheDocument()
  })

  test('falls back to 0 for non-finite values', () => {
    render(<OilTemp oilC={Number.NaN} />)

    expect(screen.getByText('0')).toBeInTheDocument()
  })

  test('applies className to root element', () => {
    const { container } = render(<OilTemp oilC={100} className="oil-test" />)

    expect(container.firstChild).toHaveClass('oil-test')
  })
})
