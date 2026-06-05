import { Box } from '@mui/material'
import { Telltale } from './Telltale'
import { TurnArrow } from './TurnArrow'
import { HazardGlyph, HighBeamGlyph, LowBeamGlyph, ParkingBrakeGlyph } from './telltaleIcons'

// Standardised cluster signal colours (ECE R121).
const GREEN = '#19e04b'
const BLUE = '#2b8cff'
const RED = '#ff3b30'

export type TelltaleBarProps = {
  /** Low-beam / position lights. */
  lights?: boolean
  /** Main / high beam. */
  highBeam?: boolean
  /** Parking brake engaged. */
  parkingBrake?: boolean
  turn?: 'none' | 'left' | 'right'
  hazards?: boolean
  /** Outside / ambient temperature in °C (shown top-right, hidden if absent). */
  ambientC?: number
  /** Lamp box size in px (turn arrows scale up from this). */
  size?: number
}

/**
 * Full-width top telltale bar: turn arrows pinned to the outer edges (the conventional cluster
 * placement), the static lamps grouped in the centre (low beam green, high beam blue, hazard
 * triangle red, parking brake red). Keeping everything up top frees the centre of the cluster
 * for the map / video / nav. Further telltales (ABS, airbag, brake, …) slot into the centre
 * group as their telemetry fields land; ECE R121 standardises their symbol/colour, not position.
 */
export function TelltaleBar({
  lights,
  highBeam,
  parkingBrake,
  turn,
  hazards,
  ambientC,
  size = 30
}: TelltaleBarProps) {
  const arrowSize = Math.round(size * 1.7)
  const hasTemp = typeof ambientC === 'number' && Number.isFinite(ambientC)
  return (
    <Box
      sx={{
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        width: '100%'
      }}
    >
      <TurnArrow side="left" active={hazards === true || turn === 'left'} size={arrowSize} />

      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          gap: `${Math.round(size * 0.7)}px`
        }}
      >
        <Telltale active={lights === true} color={GREEN} size={size} testId="tt-lowbeam">
          <LowBeamGlyph />
        </Telltale>
        <Telltale active={highBeam === true} color={BLUE} size={size} testId="tt-highbeam">
          <HighBeamGlyph />
        </Telltale>
        <Telltale active={hazards === true} color={RED} size={size} blink testId="tt-hazard">
          <HazardGlyph />
        </Telltale>
        <Telltale active={parkingBrake === true} color={RED} size={size} testId="tt-parkingbrake">
          <ParkingBrakeGlyph />
        </Telltale>
      </Box>

      {hasTemp && (
        <Box
          data-testid="ambient-temp"
          sx={{
            position: 'absolute',
            right: `${arrowSize + Math.round(size * 1.4)}px`,
            top: '50%',
            transform: 'translateY(-50%)',
            fontSize: `${Math.round(size * 0.66)}px`,
            fontWeight: 500,
            letterSpacing: 0.5,
            color: 'text.primary',
            whiteSpace: 'nowrap'
          }}
        >
          {Math.round(ambientC as number)}°C
        </Box>
      )}

      <TurnArrow side="right" active={hazards === true || turn === 'right'} size={arrowSize} />
    </Box>
  )
}
