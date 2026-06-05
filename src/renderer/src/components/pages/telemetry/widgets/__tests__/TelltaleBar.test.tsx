import { render, screen } from '@testing-library/react'
import { TelltaleBar } from '../TelltaleBar'

// Lit phase, so static lamps show solid and the turn arrows are on this half-cycle.
jest.mock('../../hooks/useBlink', () => ({ useBlink: () => true }))

function opacityOf(testId: string): number {
  return Number(screen.getByTestId(testId).style.opacity)
}

describe('TelltaleBar', () => {
  test('lights only the lamps whose signal is active', () => {
    render(<TelltaleBar lights={true} highBeam={false} parkingBrake={true} turn="none" />)
    expect(opacityOf('tt-lowbeam')).toBe(1)
    expect(opacityOf('tt-highbeam')).toBe(0)
    expect(opacityOf('tt-parkingbrake')).toBe(1)
  })

  test('high beam lamp lights when highBeam is set', () => {
    render(<TelltaleBar highBeam={true} />)
    expect(opacityOf('tt-highbeam')).toBe(1)
    expect(opacityOf('tt-lowbeam')).toBe(0)
    expect(opacityOf('tt-parkingbrake')).toBe(0)
  })

  test('forwards turn state to the outer arrows only', () => {
    render(<TelltaleBar turn="left" />)
    expect(opacityOf('turn-left')).toBe(1)
    expect(opacityOf('turn-right')).toBe(0)
    expect(opacityOf('tt-hazard')).toBe(0)
  })

  test('hazards light both outer arrows and the centre triangle', () => {
    render(<TelltaleBar hazards={true} turn="none" />)
    expect(opacityOf('turn-left')).toBe(1)
    expect(opacityOf('turn-right')).toBe(1)
    expect(opacityOf('tt-hazard')).toBe(1)
  })
})
