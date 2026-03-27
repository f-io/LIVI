import { render, screen } from '@testing-library/react'
import { Gear } from '../Gear'

describe('Gear', () => {
  test('renders default gear D when no gear is provided', () => {
    render(<Gear />)

    expect(screen.getByText('D')).toBeInTheDocument()
    expect(screen.getByText('GEAR')).toBeInTheDocument()
  })

  test('renders provided numeric gear', () => {
    render(<Gear gear={3} />)

    expect(screen.getByText('3')).toBeInTheDocument()
  })

  test('normalizes lowercase gear string to uppercase', () => {
    render(<Gear gear="d" />)

    expect(screen.getByText('D')).toBeInTheDocument()
  })

  test('trims surrounding whitespace', () => {
    render(<Gear gear="  r  " />)

    expect(screen.getByText('R')).toBeInTheDocument()
  })

  test('maps NEUTRAL to N', () => {
    render(<Gear gear="NEUTRAL" />)

    expect(screen.getByText('N')).toBeInTheDocument()
  })

  test('maps REVERSE to R', () => {
    render(<Gear gear="REVERSE" />)

    expect(screen.getByText('R')).toBeInTheDocument()
  })

  test('maps PARK to P', () => {
    render(<Gear gear="PARK" />)

    expect(screen.getByText('P')).toBeInTheDocument()
  })

  test('maps DRIVE to D', () => {
    render(<Gear gear="DRIVE" />)

    expect(screen.getByText('D')).toBeInTheDocument()
  })

  test('maps WINTER to W', () => {
    render(<Gear gear="WINTER" />)

    expect(screen.getByText('W')).toBeInTheDocument()
  })

  test('maps SPORT to S', () => {
    render(<Gear gear="SPORT" />)

    expect(screen.getByText('S')).toBeInTheDocument()
  })

  test('renders dash for empty string', () => {
    render(<Gear gear="" />)

    expect(screen.getByText('—')).toBeInTheDocument()
  })

  test('renders dash for UNKNOWN', () => {
    render(<Gear gear="UNKNOWN" />)

    expect(screen.getByText('—')).toBeInTheDocument()
  })

  test('renders dash for undefined gear explicitly passed', () => {
    render(<Gear gear={undefined} />)

    expect(screen.getByText('D')).toBeInTheDocument()
  })

  test('applies className to root element', () => {
    const { container } = render(<Gear gear="P" className="gear-test" />)

    expect(container.firstChild).toHaveClass('gear-test')
  })

  test('renders dash for null gear', () => {
    render(<Gear gear={null as unknown as string} />)

    expect(screen.getByText('—')).toBeInTheDocument()
  })
})
