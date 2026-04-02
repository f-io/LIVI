import { render, screen } from '@testing-library/react'
import { CoolantTemp } from '../CoolantTemp'
import { FuelLevel } from '../FuelLevel'
import { Gear } from '../Gear'
import { OilTemp } from '../OilTemp'
import { Rpm } from '../Rpm'
import { Speed } from '../Speed'

jest.mock('../../components/SegmentDisplay', () => ({
  SegmentDisplay: ({ value, digits }: { value: string; digits: number }) => (
    <div data-testid="segment-display">
      {value}|{digits}
    </div>
  )
}))

jest.mock('../../components/MetricRow', () => ({
  MetricRow: (props: unknown) => <div data-testid="metric-row">{JSON.stringify(props)}</div>
}))

describe('Telemetry widgets', () => {
  test('Gear normalizes known and unknown values', () => {
    const { rerender } = render(<Gear gear="neutral" />)
    expect(screen.getByText('N')).toBeInTheDocument()

    rerender(<Gear gear="unknown" />)
    expect(screen.getByText('—')).toBeInTheDocument()

    rerender(<Gear gear=" sport " />)
    expect(screen.getByText('S')).toBeInTheDocument()
  })

  test('Speed rounds and clamps value before rendering SegmentDisplay', () => {
    const { rerender } = render(<Speed speedKph={12.6} />)
    expect(screen.getByTestId('segment-display')).toHaveTextContent('13|3')

    rerender(<Speed speedKph={1400} />)
    expect(screen.getByTestId('segment-display')).toHaveTextContent('999|3')
  })

  test('Rpm rounds and clamps value before rendering SegmentDisplay', () => {
    const { rerender } = render(<Rpm rpm={3210.4} />)
    expect(screen.getByTestId('segment-display')).toHaveTextContent('3210|4')

    rerender(<Rpm rpm={12000} />)
    expect(screen.getByTestId('segment-display')).toHaveTextContent('9999|4')
  })

  test('FuelLevel maps incoming value to MetricRow props', () => {
    render(<FuelLevel fuelPct={11.8} />)
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"label":"FUEL"')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"value":12')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"warnBelow":11')
  })

  test('CoolantTemp maps and clamps to MetricRow props', () => {
    render(<CoolantTemp coolantC={9999} />)
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"label":"COOLANT"')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"value":999')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"warnFrom":110')
  })

  test('OilTemp handles non-finite value as 0', () => {
    render(<OilTemp oilC={Number.NaN} />)
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"label":"OIL"')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"value":0')
    expect(screen.getByTestId('metric-row')).toHaveTextContent('"warnFrom":130')
  })
})
