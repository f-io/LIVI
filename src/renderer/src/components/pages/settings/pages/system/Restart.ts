export function Restart(): null {
  window.carplay.quit().catch(console.error)
  return null
}
