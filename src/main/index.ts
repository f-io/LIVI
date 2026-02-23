import { app } from 'electron'
import { loadConfig } from './config/loadConfig'
import { createMainWindow } from './window/createWindow'
import './app/gpu'
import { setupLifecycle } from '@main/app/lifecycle'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { registerIpc } from '@main/ipc'
import { runtimeStateProps } from '@main/types'
import { saveSettings } from '@main/ipc/utils'
import { Socket } from '@main/services/Socket'
import { setupAppIdentity } from '@main/app/init'

app.whenReady().then(() => {
  const runtimeState: runtimeStateProps = {
    config: loadConfig(),
    socket: null,
    isQuitting: false,
    suppressNextFsSync: false
  }
  runtimeState.socket = new Socket(runtimeState.config, (next) => saveSettings(runtimeState, next))

  setupAppIdentity()
  registerAppProtocol()
  registerIpc(runtimeState)
  createMainWindow(runtimeState)
  setupLifecycle(runtimeState)
})
