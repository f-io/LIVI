const BENIGN_USB_PATTERNS: RegExp[] = [
  /udev/i,
  /LIBUSB_ERROR_NO_DEVICE/i,
  /LIBUSB_ERROR_NOT_FOUND/i,
  /LIBUSB_TRANSFER_NO_DEVICE/i,
  /LIBUSB_TRANSFER_ERROR/i,
  /LIBUSB_ERROR_BUSY/i,
  /No such device/i,
  /device has been disconnected/i,
  /transferIn error/i
]

function describe(err: unknown): string {
  if (err instanceof Error) {
    return `${err.message}\n${err.stack ?? ''}`
  }
  try {
    return String(err)
  } catch {
    return '<unprintable error>'
  }
}

function isBenignUsbError(err: unknown): boolean {
  const text = describe(err)
  return BENIGN_USB_PATTERNS.some((re) => re.test(text))
}

let installed = false

export function installMainProcessErrorHandlers(): void {
  if (installed) return
  installed = true

  process.on('uncaughtException', (err) => {
    if (isBenignUsbError(err)) {
      console.warn('[errorHandler] suppressed USB teardown noise:', describe(err))
      return
    }
    console.error('[errorHandler] uncaughtException:', describe(err))
  })

  process.on('unhandledRejection', (reason) => {
    if (isBenignUsbError(reason)) {
      console.warn('[errorHandler] suppressed USB teardown rejection:', describe(reason))
      return
    }
    console.error('[errorHandler] unhandledRejection:', describe(reason))
  })
}
