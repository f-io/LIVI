let replaced = false

/** Called when a run put a new helper binary in place of the staged one. */
export function markHelperRestaged(): void {
  replaced = true
}

/** Whether the staged helper changed in this run. The AP service still runs the old one. */
export function helperRestaged(): boolean {
  return replaced
}

/** Called once the AP service was restarted onto the new binary. */
export function clearHelperRestaged(): void {
  replaced = false
}
