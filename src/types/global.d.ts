export {}

declare global {
  interface Window {
    carplay: {
      ipc: {
        sendKeyCommand: (cmd: string) => void
      }
      usb: {
        listenForEvents: (...args: any[]) => void
        unlistenForEvents: (...args: any[]) => void
      }
    }
  }
}
