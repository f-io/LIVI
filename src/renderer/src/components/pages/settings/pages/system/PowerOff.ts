let fired = false

export function PowerOff(): null {
  if (fired) return null
  fired = true
  window.app.quitApp().catch(console.error)
  return null
}

// Test-only: reset the one-shot guard between cases.
export function __resetPowerOffGuardForTests(): void {
  fired = false
}
