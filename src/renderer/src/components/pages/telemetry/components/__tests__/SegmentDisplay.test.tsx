import { render } from '@testing-library/react'
import { SegmentDisplay } from '../SegmentDisplay'

describe('SegmentDisplay', () => {
  test('renders 7 polygons per digit', () => {
    const { container } = render(<SegmentDisplay value="12" digits={2} />)
    expect(container.querySelectorAll('polygon')).toHaveLength(14)
  })

  test('uses transparent off segments in blank mode for unknown chars', () => {
    const { container } = render(
      <SegmentDisplay value="A?" digits={2} offMode="blank" offColor="rgb(1, 2, 3)" />
    )
    const polygons = Array.from(container.querySelectorAll('polygon'))
    expect(polygons.every((p) => p.getAttribute('fill') === 'transparent')).toBe(true)
  })

  test('uses offColor in dim mode for blank chars', () => {
    const { container } = render(
      <SegmentDisplay value="A?" digits={2} offMode="dim" offColor="rgb(1, 2, 3)" />
    )
    const polygons = Array.from(container.querySelectorAll('polygon'))
    expect(polygons.every((p) => p.getAttribute('fill') === 'rgb(1, 2, 3)')).toBe(true)
  })

  test('dims leading zeros when enabled', () => {
    const { container } = render(
      <SegmentDisplay
        value="007"
        digits={3}
        dimLeadingZeros
        leadingZeroColor="rgb(8, 8, 8)"
        onColor="rgb(9, 9, 9)"
      />
    )

    const polygons = Array.from(container.querySelectorAll('polygon'))
    const fills = polygons.map((p) => p.getAttribute('fill'))

    expect(fills).toContain('rgb(8, 8, 8)')
    expect(fills).toContain('rgb(9, 9, 9)')
  })
})
