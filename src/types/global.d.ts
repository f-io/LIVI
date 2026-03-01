export {}

declare global {
  interface Window {
    carplay: {
      ipc: {
        sendCommand: (cmd: string) => void
      }
      usb: {
        listenForEvents: (...args: any[]) => void
        unlistenForEvents: (...args: any[]) => void
      }
    }
  }
}
