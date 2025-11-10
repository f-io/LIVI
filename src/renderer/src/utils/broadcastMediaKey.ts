export function broadcastMediaKey(action: string) {
  window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: action } }))
}
