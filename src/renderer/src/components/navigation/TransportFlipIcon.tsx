import SyncIcon from '@mui/icons-material/Sync'

type Props = {
  active: 'dongle' | 'aa' | null
  fontSize?: number
}

export function TransportFlipIcon({ active, fontSize = 30 }: Props) {
  const letter = active === 'dongle' ? 'D' : ''

  return (
    <span
      aria-hidden={false}
      style={{
        position: 'relative',
        display: 'inline-grid',
        placeItems: 'center',
        width: fontSize,
        height: fontSize
      }}
    >
      <SyncIcon sx={{ fontSize }} />
      <span
        aria-hidden
        style={{
          position: 'absolute',
          bottom: -fontSize * 0.18,
          right: -fontSize * 0.05,
          fontWeight: 700,
          fontSize: fontSize * 0.32,
          lineHeight: 1,
          color: 'inherit',
          pointerEvents: 'none'
        }}
      >
        {letter}
      </span>
    </span>
  )
}
