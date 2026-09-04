import { stopSystemVolumeMonitor } from '@main/services/audio/SystemVolume'
import { stopPhoneSuppression } from '@main/services/gvfsPhoneGuard'
import { runPendingPowerAction } from '@main/services/power/hostPower'
import { releaseWifiApForQuit } from '@main/services/projection/driver/helper/wifiApUnit'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'
import { closeAllSecondaryWindows } from '@main/window/secondaryWindows'
import { app, BrowserWindow } from 'electron'

export function setupLifecycle(runtimeState: runtimeStateProps, services: ServicesProps) {
  const { projectionService, telemetrySocket } = services
  const mainWindow = getMainWindow()

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0 && !mainWindow)
      createMainWindow(runtimeState, services)
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
      let t: NodeJS.Timeout | undefined
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
        clearTimeout(t)
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
    const tDisconnect = 800
    const tCarplayStop = 6000
    const tWirelessShutdown = 8000

    // Global watchdog: log only
    const watchdogMs = process.platform === 'darwin' ? 10000 : 3000
    const watchdog = setTimeout(() => {
      console.warn(`[MAIN] before-quit watchdog: giving up waiting after ${watchdogMs}ms`)
    }, watchdogMs)

    try {
      closeAllSecondaryWindows()
      projectionService.beginShutdown()

      stopPhoneSuppression()
      stopSystemVolumeMonitor()

      await measureStep('projection.shutdownWirelessSessions()', async () => {
        await withTimeout(
          'projection.shutdownWirelessSessions()',
          projectionService.shutdownWirelessSessions(),
          tWirelessShutdown
        )
      })

      await measureStep('projection.disconnectPhone()', async () => {
        await withTimeout(
          'projection.disconnectPhone()',
          projectionService.disconnectPhone(),
          tDisconnect
        )
        await sleep(75)
      })

      await measureStep('projection.disconnectHostBtPhones()', async () => {
        await withTimeout(
          'projection.disconnectHostBtPhones()',
          projectionService.disconnectHostBtPhones(),
          1500
        )
      })

      await measureStep('telemetrySocket.disconnect()', async () => {
        await withTimeout(
          'telemetrySocket.disconnect()',
          telemetrySocket?.disconnect?.() ?? Promise.resolve(),
          300
        )
      })

      await measureStep('projection.stopHelper()', async () => {
        await withTimeout('projection.stopHelper()', projectionService.stopHelper(), 2500)
      })

      await measureStep('projection.stop()', async () => {
        await withTimeout('projection.stop()', projectionService.stop(), tCarplayStop)
      })

      await measureStep('wifiAp.release()', async () => {
        await withTimeout('wifiAp.release()', releaseWifiApForQuit(runtimeState.config), 2000)
      })
    } catch (err) {
      console.warn('[MAIN] Error while quitting:', err)
    } finally {
      setTimeout(() => clearTimeout(watchdog), 250)

      runPendingPowerAction()
      setImmediate(() => process.kill(process.pid, 'SIGKILL'))
    }
  })

  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit()
  })
}
