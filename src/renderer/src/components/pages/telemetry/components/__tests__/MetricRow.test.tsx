import { render, screen } from '@testing-library/react'
import { MetricRow } from '../MetricRow'

describe('MetricRow', () => {
  test('renders label, value and unit', () => {
    render(
      <MetricRow
        label="COOLANT"
        value={95}
        unit="°C"
        min={0}
        max={140}
        barValue={95}
        labelAlign="left"
      />
    )

    expect(screen.getByText('COOLANT')).toBeInTheDocument()
    expect(screen.getByText('95')).toBeInTheDocument()
    expect(screen.getByText('°C')).toBeInTheDocument()
  })

  test('handles non-finite range input safely', () => {
    render(
      <MetricRow label="FUEL" value="--" min={Number.NaN} max={Number.NaN} barValue={Number.NaN} />
    )
    expect(screen.getByText('FUEL')).toBeInTheDocument()
    expect(screen.getByText('--')).toBeInTheDocument()
  })

  test('supports center label alignment', () => {
    render(
      <MetricRow
        label="RPM"
        value={2500}
        unit=""
        min={0}
        max={8000}
        barValue={2500}
        labelAlign="center"
      />
    )

    expect(screen.getByText('RPM')).toBeInTheDocument()
    expect(screen.getByText('2500')).toBeInTheDocument()
  })
})
