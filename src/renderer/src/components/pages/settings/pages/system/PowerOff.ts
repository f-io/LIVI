export function PowerOff(): null {
  window.carplay.quit().catch(console.error)
  return null
}
