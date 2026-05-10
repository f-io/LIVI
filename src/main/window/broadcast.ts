import { getMainWindow } from '@main/window/createWindow'
import { getSecondaryWindow, type SecondaryWindowRole } from '@main/window/secondaryWindows'
import type { WebContents } from 'electron'

const SECONDARY_ROLES: SecondaryWindowRole[] = ['dash', 'aux']

export function getAllRendererWebContents(): WebContents[] {
  const out: WebContents[] = []
  const main = getMainWindow()
  if (main && !main.isDestroyed()) out.push(main.webContents)
  for (const role of SECONDARY_ROLES) {
    const w = getSecondaryWindow(role)
    if (w && !w.isDestroyed()) out.push(w.webContents)
  }
  return out
}

export function getSecondaryRendererWebContents(): WebContents[] {
  const out: WebContents[] = []
  for (const role of SECONDARY_ROLES) {
    const w = getSecondaryWindow(role)
    if (w && !w.isDestroyed()) out.push(w.webContents)
  }
  return out
}

export function broadcastToRenderers(channel: string, ...args: unknown[]): void {
  for (const wc of getAllRendererWebContents()) {
    try {
      wc.send(channel, ...args)
    } catch (e) {
      console.warn(`[broadcast] send failed on '${channel}' (ignored)`, e)
    }
  }
}

export function broadcastToSecondaryRenderers(channel: string, ...args: unknown[]): void {
  for (const wc of getSecondaryRendererWebContents()) {
    try {
      wc.send(channel, ...args)
    } catch (e) {
      console.warn(`[broadcast] send failed on '${channel}' (ignored)`, e)
    }
  }
}
