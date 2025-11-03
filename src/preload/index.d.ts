import { ElectronAPI } from '@electron-toolkit/preload'
import { Api } from './index'

declare global {
  interface Window {
    electron: ElectronAPI
    api: Api
    electronAPI: Api
  }
}
