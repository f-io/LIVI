export function broadcastMediaKey(action: string) {
  const send = window.app?.broadcastMediaKey
  if (typeof send === 'function') {
    try {
      send(action)
      return
    } catch {}
  }
  window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: action } }))
}
