import { render } from '@testing-library/react'
import { ViewAreaMask } from '../ViewAreaMask'

describe('ViewAreaMask', () => {
  const insets = { top: 10, bottom: 20, left: 5, right: 15 }

  test('renders nothing when not visible', () => {
    const { container } = render(
      <ViewAreaMask visible={false} displayWidth={800} displayHeight={480} insets={insets} />
    )

    expect(container).toBeEmptyDOMElement()
  })

  test('renders nothing when displayWidth is zero or negative', () => {
    const { container } = render(
      <ViewAreaMask visible displayWidth={0} displayHeight={480} insets={insets} />
    )

    expect(container).toBeEmptyDOMElement()
  })

  test('renders nothing when displayHeight is zero or negative', () => {
    const { container } = render(
      <ViewAreaMask visible displayWidth={800} displayHeight={-1} insets={insets} />
    )

    expect(container).toBeEmptyDOMElement()
  })

  test('renders four inset bars with percentage sizing when visible', () => {
    const { container } = render(
      <ViewAreaMask visible displayWidth={100} displayHeight={200} insets={insets} />
    )

    const bars = Array.from(container.children) as HTMLElement[]
    expect(bars).toHaveLength(4)

    const [top, bottom, left, right] = bars
    expect(top.style.height).toBe('5%')
    expect(bottom.style.height).toBe('10%')
    expect(left.style.width).toBe('5%')
    expect(right.style.width).toBe('15%')

    bars.forEach((bar) => {
      expect(bar.style.position).toBe('absolute')
      expect(bar.style.pointerEvents).toBe('none')
      expect(bar.style.zIndex).toBe('5')
    })
  })

  test('clamps negative insets to zero percent', () => {
    const { container } = render(
      <ViewAreaMask
        visible
        displayWidth={100}
        displayHeight={200}
        insets={{ top: -50, bottom: 0, left: -10, right: 0 }}
      />
    )

    const [top, , left] = Array.from(container.children) as HTMLElement[]
    expect(top.style.height).toBe('0%')
    expect(left.style.width).toBe('0%')
  })
})
