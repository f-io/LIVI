import { render } from '@testing-library/react'
import { RpmRing } from '../RpmRing'

describe('RpmRing', () => {
  test('renders one tick per configured count', () => {
    const { container } = render(
      <RpmRing
        rpm={0}
        maxRpm={7000}
        redlineRpm={6000}
        ticks={50}
        colorOff="#111"
        colorOn="#0f0"
        colorRedline="#f00"
      />
    )
    expect(container.querySelectorAll('rect')).toHaveLength(50)
  })

  test('colors active and redline ticks correctly', () => {
    const { container } = render(
      <RpmRing
        rpm={7000}
        maxRpm={7000}
        redlineRpm={5000}
        ticks={20}
        colorOff="#111"
        colorOn="#0f0"
        colorRedline="#f00"
      />
    )
    const fills = Array.from(container.querySelectorAll('rect')).map((x) => x.getAttribute('fill'))

    expect(fills).toContain('#0f0')
    expect(fills).toContain('#f00')
    expect(fills).not.toContain('#111')
  })

  test('sanitizes invalid max/ticks values', () => {
    const { container } = render(
      <RpmRing
        rpm={10}
        maxRpm={0}
        redlineRpm={-5}
        ticks={1}
        colorOff="#111"
        colorOn="#0f0"
        colorRedline="#f00"
      />
    )
    expect(container.querySelectorAll('rect')).toHaveLength(2)
  })

  test('clamps redlineRpm above maxRpm down to maxRpm', () => {
    const { container } = render(
      <RpmRing
        rpm={7000}
        maxRpm={7000}
        redlineRpm={9000}
        ticks={20}
        colorOff="#111"
        colorOn="#0f0"
        colorRedline="#f00"
      />
    )

    const fills = Array.from(container.querySelectorAll('rect')).map((x) => x.getAttribute('fill'))

    expect(fills).toContain('#0f0')
    expect(fills).not.toContain('#f00')
  })

  test('uses default tick count when ticks is not provided', () => {
    const { container } = render(
      <RpmRing
        rpm={0}
        maxRpm={7000}
        redlineRpm={6000}
        colorOff="#111"
        colorOn="#0f0"
        colorRedline="#f00"
      />
    )

    expect(container.querySelectorAll('rect')).toHaveLength(44)
  })
})
