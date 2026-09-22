import { registerIpcHandle } from '@main/ipc/register'

/** USB-related renderer queries not tied to a projection session. */
export function registerUsbIpc(): void {
  registerIpcHandle('get-sysdefault-mic-label', () => 'system default')
}
