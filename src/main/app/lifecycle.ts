import { app, BrowserWindow } from 'electron'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'

export function setupLifecycle(runtimeState: runtimeStateProps, services: ServicesProps) {
  const { carplayService, usbService, telemetrySocket } = services
  const mainWindow = getMainWindow()

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0 && !mainWindow) createMainWindow(runtimeState)
    else mainWindow?.show()
  })

  app.on('before-quit', async (e) => {
    if (runtimeState.isQuitting) return
    runtimeState.isQuitting = true
    e.preventDefault()

    const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms))

    const withTimeout = async <T>(
      label: string,
      p: Promise<T>,
      ms: number
    ): Promise<T | undefined> => {
      let t: NodeJS.Timeout | null = null
      try {
        return (await Promise.race([
          p,
          new Promise<T | undefined>((resolve) => {
            t = setTimeout(() => {
              console.warn(`[MAIN] before-quit timeout: ${label} after ${ms}ms`)
              resolve(undefined)
            }, ms)
          })
        ])) as T | undefined
      } finally {
        if (t) clearTimeout(t)
      }
    }

    const measureStep = async (label: string, fn: () => Promise<unknown>) => {
      const t0 = Date.now()
      console.log(`[MAIN] before-quit step:start ${label}`)
      try {
        await fn()
      } finally {
        console.log(`[MAIN] before-quit step:done ${label} (${Date.now() - t0}ms)`)
      }
    }

    // Safeguards based on measured timings
    const tUsbStop = 500
    const tDisconnect = 800
    const tCarplayStop = 6000

    // Global watchdog: log only
    // TODO replace by isMac fn
    const watchdogMs = process.platform === 'darwin' ? 10000 : 3000
    const watchdog = setTimeout(() => {
      console.warn(`[MAIN] before-quit watchdog: giving up waiting after ${watchdogMs}ms`)
    }, watchdogMs)

    try {
      carplayService.beginShutdown()

      // Block hotplug callbacks ASAP
      usbService?.beginShutdown()

      await measureStep('usbService.stop()', async () => {
        await withTimeout('usbService.stop()', usbService?.stop?.() ?? Promise.resolve(), tUsbStop)
      })

      await measureStep('carplay.disconnectPhone()', async () => {
        await withTimeout(
          'carplay.disconnectPhone()',
          carplayService.disconnectPhone(),
          tDisconnect
        )
        await sleep(75)
      })

      await measureStep('telemetrySocket.disconnect()', async () => {
        await withTimeout(
          'telemetrySocket.disconnect()',
          telemetrySocket?.disconnect?.() ?? Promise.resolve(),
          300
        )
      })

      await measureStep('carplay.stop()', async () => {
        await withTimeout('carplay.stop()', carplayService.stop(), tCarplayStop)
      })
    } catch (err) {
      console.warn('[MAIN] Error while quitting:', err)
    } finally {
      setTimeout(() => clearTimeout(watchdog), 250)
      setImmediate(() => app.quit())
    }
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })
}
