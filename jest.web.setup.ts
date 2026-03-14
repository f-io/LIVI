import '@testing-library/jest-dom'
import { TextDecoder, TextEncoder } from 'util'

if (typeof globalThis.TextEncoder === 'undefined') {
  Object.defineProperty(globalThis, 'TextEncoder', {
    configurable: true,
    writable: true,
    value: TextEncoder
  })
}

if (typeof globalThis.TextDecoder === 'undefined') {
  Object.defineProperty(globalThis, 'TextDecoder', {
    configurable: true,
    writable: true,
    value: TextDecoder
  })
}

if (typeof globalThis.structuredClone === 'undefined') {
  ;(
    globalThis as typeof globalThis & {
      structuredClone: <T>(value: T) => T
    }
  ).structuredClone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T
}
