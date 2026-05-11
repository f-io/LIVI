let fired = false

export function Restart(): null {
  if (fired) return null
  fired = true
  window.app.restartApp().catch(console.error)
  return null
}

// Test-only: reset the one-shot guard between cases.
export function __resetRestartGuardForTests(): void {
  fired = false
}
