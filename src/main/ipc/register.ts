import { ipcMain, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'

type InvokeHandler<TArgs extends unknown[] = unknown[], TResult = unknown> = (
  event: IpcMainInvokeEvent,
  ...args: TArgs
) => TResult | Promise<TResult>

type EventListener<TArgs extends unknown[] = unknown[]> = (
  event: IpcMainEvent,
  ...args: TArgs
) => void

export function registerIpcHandle<TArgs extends unknown[] = unknown[], TResult = unknown>(
  channel: string,
  handler: InvokeHandler<TArgs, TResult>
): void {
  ipcMain.removeHandler(channel)
  ipcMain.handle(channel, handler as Parameters<typeof ipcMain.handle>[1])
}

export function registerIpcOn<TArgs extends unknown[] = unknown[]>(
  channel: string,
  listener: EventListener<TArgs>
): void {
  ipcMain.removeAllListeners(channel)
  ipcMain.on(channel, listener as Parameters<typeof ipcMain.on>[1])
}
