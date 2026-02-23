import { app } from 'electron'
import { USBService } from '@main/services/usb/USBService'
import { runtimeStateProps } from '@main/types'

export function setupLifecycle({ isQuitting }: runtimeStateProps) {
  let usbService: USBService | null = null

  if (carplayService) {
    usbService = new USBService(carplayService)
  }

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('before-quit', async (e) => {
    if (isQuitting) return
    isQuitting = true
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
    const watchdogMs = process.platform === 'darwin' ? 10000 : 3000
    const watchdog = setTimeout(() => {
      console.warn(`[MAIN] before-quit watchdog: giving up waiting after ${watchdogMs}ms`)
    }, watchdogMs)

    try {
      ;(carplayService as any).shuttingDown = true

      // Block hotplug callbacks ASAP
      usbService?.beginShutdown()

      await measureStep('usbService.stop()', async () => {
        await withTimeout('usbService.stop()', usbService?.stop?.() ?? Promise.resolve(), tUsbStop)
      })

      await measureStep('carplay.disconnectPhone()', async () => {
        if (carplayService) {
          await withTimeout(
            'carplay.disconnectPhone()',
            carplayService.disconnectPhone(),
            tDisconnect
          )
        }

        await sleep(75)
      })

      await measureStep('carplay.stop()', async () => {
        if (carplayService) {
          await withTimeout('carplay.stop()', carplayService.stop(), tCarplayStop)
        }
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
