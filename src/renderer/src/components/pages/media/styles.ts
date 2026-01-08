export function circleBtnStyle(
  size: number,
  pressed = false,
  focused = false
): React.CSSProperties {
  return {
    position: 'relative',
    width: size,
    height: size,
    borderRadius: '50%',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    border: 'none',
    background: 'rgba(255,255,255,0.18)',
    cursor: 'pointer',
    userSelect: 'none',
    WebkitTapHighlightColor: 'transparent',
    lineHeight: 0,
    outline: 'none',
    transform: pressed ? 'scale(0.94)' : 'scale(1)',
    transition: 'transform 110ms ease, box-shadow 110ms ease, background 110ms ease',
    boxShadow: focused
      ? '0 0 0 3px rgba(255,255,255,0.55)'
      : pressed
        ? '0 0 0 5px rgba(255,255,255,0.35) inset'
        : 'none'
  }
}
