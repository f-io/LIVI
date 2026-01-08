export {}

declare global {
  interface NetworkInformation extends EventTarget {
    type?: string
    effectiveType?: string
    addEventListener(type: 'change', listener: () => void, options?: any): void
    removeEventListener(type: 'change', listener: () => void, options?: any): void
  }

  interface Navigator {
    connection?: NetworkInformation
    mozConnection?: NetworkInformation
    webkitConnection?: NetworkInformation
  }
}
